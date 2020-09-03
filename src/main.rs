use hyphenation::{
    Hyphenator,Language, Load, Standard,
};
use quick_xml::{
    Reader, events::Event
};
use termion::{
    event::Key, input::TermRead, raw::IntoRawMode,
    style,color,terminal_size
};
use clap::{
    Arg,app_from_crate,
    crate_name,crate_version,crate_authors,crate_description
};
use anyhow::{
    Context
};
use serde::{
    Serialize, Deserialize
};
use std::{
    io::{BufRead, Write, stdout, stdin},
    mem, collections::BTreeMap
};
#[macro_use]
extern crate lazy_static;




#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, PartialOrd)]
struct BookState {
    tag_count: usize,
    word_offset: usize,
}

#[derive(Serialize, Deserialize)]
struct TBconfig {
    books: BTreeMap<String, BookState>,
}

#[derive(Debug)]
struct Line {
    // If the line is coming from the FB2 file, then there should
    // be Some(offset) indicating the source position.  Otherwise
    // if the line is inserted by further postprocessing, its source
    // is None.
    xml_offset: Option<BookState>,
    content: String
}

#[derive(Debug)]
enum Align {
    Left,
    Center,
    Right
}

#[derive(Hash, Eq, PartialEq, Debug)]
enum FBstyle {
    Bold,
    Strong,
    Title,
    Subtitle,
    Emph,
    // Add more stuff
}


#[derive(Debug)]
struct WriterState {
    // Count the processed lines.  We use this to control how much
    // input we need to read in order to fill the screen or print
    // so many lines.
    pub line : usize,
    // Max width of the line on the screen
    pub line_width: usize,
    // Current buffer line we are adding words to
    pub l: String,
    // Position in our buffer line
    pub pos: usize,
    // Lines we have read so far
    pub lines: Vec<Line>,
    // Did we reach the end of file
    pub eof: bool,
    // We use these fields to annotate the lines with their positions
    // in the xml document, so that we could restore it on the next load.
    pub xml_offset: BookState,
    // XXX DEBUG ONLY. We want to keep the collection of tags that we
    // are skipping.  When the collection will become empty, all tags
    // are handled.
    pub tags: std::collections::HashSet::<String>,
    // Constant prefix we are using for lists, epigraphs, etc.
    pub prefix: String,
    pub needs_prefix: bool,
    // What is the current alignment
    pub align: Align,
    // Mapping of the book styles to escape sequences.
    pub smap: std::collections::HashMap<FBstyle,(String,String)>,
    // A list of the open style tags so far.
    pub styles: Vec<FBstyle>,
    // Are we outputing the title right now
    pub in_title: bool,
    // TODO implement parsing of a description and get rid of this field.
    pub skip: bool,
    pub last_line_empty: bool,
    // Do we expect the next paragraph to come to be the first one in
    // the section, body, etc.  This impacts whether we add indent in the
    // beginning of it.
    pub first_paragraph: bool,
}



impl WriterState {
    fn line_done(&mut self) {
        let mut t = mem::replace(&mut self.l, String::from(""));
        // TODO this is not correct, as we store the xml offset that
        // occurs at the *end* of the line, not at the beginning...
        let o = self.xml_offset;

        let s = self.line_width - self.pos;
        // We might have not yet inserted the prefix, in which case
        // we are done here.
        if t.len() == 0 {
            self.lines.push(Line {xml_offset: Some(o), content: t});
            self.pos = 0;
            self.needs_prefix = true;
            self.last_line_empty = true;
            return
        }

        match self.align {
            Align::Right => {
                //print!("pr = {}; t= {}; s = {}\r\n", self.prefix, t, s);
                let q = " ".to_owned().repeat(s);
                if s > 0 {
                t.insert_str(self.prefix.chars().count(), &q);
                }
            }
            Align::Center => {
                let q = " ".to_owned().repeat(s/2);
                t.insert_str(self.prefix.chars().count(), &q);
            }
            _ => ()
        }
        // Close all the styles for the given line
        for s in self.styles.iter().rev() {
            if let Some(s) = self.smap.get(&s) {
                t.push_str(&s.1);
            }
        }
        self.lines.push(Line {xml_offset: Some(o), content: t});
        self.pos = 0;
        self.needs_prefix = true;
        self.last_line_empty = false;
    }
    fn _dprint(&self) {
        print!("line: {}, pos: {}, eof: {}", self.line, self.pos, self.eof);
    }
    fn chars_left(&self) -> usize {
        if self.pos > self.line_width {
            print!("\r\n{}\r\n", self.l);
            panic!("pos > line-width {} {}", self.pos, self.line_width);
        }
        self.line_width - self.pos
    }
    fn push_empty_line(&mut self) {
        self.lines.push(Line {xml_offset: None, content: String::from("")});
        self.last_line_empty = true;
    }
    fn ensure_empty_line(&mut self) {
        // Make sure that we are done with what we have
        self.ensure_new_line();
        // Push the new line if it is not there yet
        if !self.last_line_empty {
            self.push_empty_line();
        }
    }
    fn ensure_new_line(&mut self) {
        if !self.needs_prefix {
            self.line_done();
        }
    }
    fn change_prefix(&mut self, p: &str){ //String) {
        if self.pos != 0 { self.line_done(); }
        self.line_width += self.prefix.chars().count();
        self.prefix = p.to_string();
        self.line_width -= self.prefix.chars().count();
    }

    fn push_word(&mut self, w: &str) {
        if self.needs_prefix {
            self.l.push_str(&self.prefix);
            // push all the styles in the beginning of the line
            for s in &self.styles {
                if let Some(s) = self.smap.get(&s) {
                  self.l.push_str(&s.0);
                }
            }
            self.needs_prefix = false;
        }
        if self.in_title {
            self.l.push_str(&w.to_uppercase());
        } else  {
            self.l.push_str(&w);
        }
        // XXX we often know the length of the string, as we sometimes
        // check whether the word would fit into the remaining line...
        // So this is a small source of inefficiency.
        self.pos += w.chars().count();
    }
    fn push_fmt_start(&mut self, w: FBstyle) {
        if let Some(s) = self.smap.get(&w) {
          self.l.push_str(&s.0);
          self.styles.push(w);
        }
    }

    fn push_fmt_end(&mut self, w: FBstyle) {
        if let Some(s) = self.smap.get(&w) {
          self.l.push_str(&s.1);
          let s = self.styles.pop();
          // XXX this assertion is for debugging to ensure that our
          // open/closing tags are consistent.
          assert_eq!(s, Some(w));
        }
    }
}

#[derive(Debug, Clone)]
struct ProcessingError {
    desc: String,
}

impl ProcessingError {
    fn new(msg: &str) -> ProcessingError {
        ProcessingError{desc: msg.to_string()}
    }
}

impl std::fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f,"{}", self.desc)
    }
}impl std::error::Error for ProcessingError {
    fn description(&self) -> &str {
        &self.desc
    }
}

trait OutText {
    fn out (&self, s: &str, state: &mut WriterState) -> anyhow::Result<()>;
}

impl OutText for Standard {
    fn out (&self, s: &str, state: &mut WriterState) -> anyhow::Result<()> {
        // Sometimes we can get bogous inputs that are either empty or consist
        // only of whitespaces.
        if s.trim().len() == 0 {
            return Ok(());
        }

        if s.starts_with(" ")
           && !state.l.ends_with(" ") //&& state.l.len() != 0
           && state.chars_left() >= 1 {
            state.push_word(" ");
        }

        lazy_static! {
            static ref RE: regex::Regex = regex::Regex::new(r"(\W*)(\w*)(\W*)").unwrap();
        }
        for (i, w) in s.split_whitespace().enumerate() {
            let wlen = w.chars().count();

            let space = if i == 0 { "" } else { " " };
            if wlen + space.len() <= state.chars_left() {
                state.push_word(space);
                state.push_word(w);
            } else {
                // Note that the following regexp is used to peel off
                // punctuation from the sequence of non-whitespace caracters
                // into postfix, middle and prefix.  Otherwise the
                // hyphenator below will treat the puncutation as alphabet
                // letters resulting in weird word breaks.  While it is
                // possible to list a simple set of punctuation characters
                // manually {., ;, !, ...}, it is difficult to do this
                // consistently for all the unicode symbols.  The use of
                // regexps simply solves this problem in a reasonably cheap
                // way (as long as we don't compile regrexp all the time).
                // It is perfectly fine to reconsider this decision later
                // in case we hit a niticeable performance penalty.
                let caps = RE.captures(w).ok_or(ProcessingError::new(
                        &format!("regexp failed while recognising `{}'", w)))?;
                let wprefix = caps.get(1).ok_or(ProcessingError::new(
                        &format!("error getting caputure group 1 in `{}'", w)))?
                        .as_str();
                let wmiddle = caps.get(2).ok_or(ProcessingError::new(
                        &format!("error getting caputure group 2 in `{}'", w)))?
                        .as_str();
                let wpostfix = caps.get(3).ok_or(ProcessingError::new(
                        &format!("error getting caputure group 2 in `{}'", w)))?
                        .as_str();

                // FIXME we don't need to create vector, inline the code!
                // Hyphenate the word
                let mut triples = Vec::new();
                for n in self.hyphenate(wmiddle).breaks {
                    let (head, tail) = wmiddle.split_at(n);
                    let hyphen = if head.ends_with('-') { "" } else { "-" };
                    triples.push((head, hyphen, tail));
                }

                // FIXME sometimes the hyphenator decides to leave a
                // single letter either in the left or right parts of
                // the word, and this looks ugly.  Kill this behaviour.
                // Now iterate the tripletes
                let mut hyp_found = false;
                for &(head, hyp, tail) in triples.iter().rev() {
                    let wlen = head.chars().count() + hyp.chars().count()
                               + wprefix.chars().count() + space.len();
                    if wlen <= state.chars_left() {
                        // FIXME what if the length of the tail > line_widht?
                        assert!(tail.chars().count() + wpostfix.chars().count()
                                <= state.line_width);
                        // push space only if we are not at the first word
                        state.push_word(space);
                        state.push_word(wprefix);
                        state.push_word(head);
                        state.push_word(hyp);
                        state.line_done();

                        state.push_word(tail);
                        state.push_word(wpostfix);
                        // update xml_txt_off with the current word count `i`
                        state.xml_offset.word_offset = i;
                        hyp_found = true;
                        state.line += 1;
                        break;
                    }
                }

                // If we didn't find the hyphenation, break right here
                if !hyp_found {
                    state.line_done();
                    // update xml_txt_off with the current word count `i`
                    state.xml_offset.word_offset = i;

                    state.line += 1;
                    // If `w` is crazily long, we'll just break in the middle
                    if wlen > state.line_width {
                        // FIXME this is quite weird now, the last chunk of
                        // the `w` might be shorter than the line...
                        let v: Vec<_> = w.chars().collect();
                        for l in v.chunks (state.line_width) {
                            state.l = l.iter().collect::<String>();
                            state.line_done();
                            state.line += 1;
                        }
                    } else {
                        state.push_word(w);
                    }
                }

            }
        }

        if s.ends_with(" ") && state.chars_left() >= 1 {
            state.push_word(" ");
        }
        Ok(())
    }
}


fn crank<B: BufRead> (reader : &mut Reader<B>,
                      hyphenator: &Standard,
                      ws : &mut WriterState,
                      // how many lines do we accumulate
                      count : usize) -> anyhow::Result<()> {

    let mut buf = Vec::new();

    let l = ws.line + count;
    while !ws.eof && ws.line < l {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => {
                match e.name() {

                    b"binary" | b"description" => { ws.skip = true; }
                    b"p" => {
                        if !ws.in_title && !ws.first_paragraph {
                            ws.ensure_new_line();
                            ws.push_word("    ");
                        }
                    }
                    b"v" => {
                        if !ws.in_title {
                            ws.push_word("        ");
                        }
                    }
                    b"stanza" | b"section" => (),
                    b"poem" => {
                        ws.ensure_empty_line();
                    }
                    b"epigraph" => {
                        ws.align = Align::Right;
                        ws.change_prefix("                 ")
                    }
                    b"cite" => {
                        ws.ensure_empty_line();
                        ws.change_prefix("                 ")
                    }
                    b"emphasis" => {
                        ws.push_fmt_start(FBstyle::Emph);
                    }
                    b"strong" => {
                        ws.push_fmt_start(FBstyle::Strong);
                    }
                    b"title" => {
                        ws.ensure_empty_line();
                        ws.push_fmt_start(FBstyle::Title);
                        ws.in_title = true;
                    }
                    b"subtitle" => {
                        ws.ensure_empty_line();
                        ws.push_fmt_start(FBstyle::Subtitle);
                        ws.push_word(&"§ ".to_string());
                        //ws.in_title = true;
                    }
                    b"text-author" => {
                        // XXX we assume that we don't have  nesting here.
                        // otherwise we need to have a stack of aligns...
                        ws.align = Align::Right;
                        ws.ensure_new_line();
                        ws.push_word(&"– ".to_string());
                    }

                    _ => {
                        if !ws.skip {
                            ws.tags.insert(
                                std::str::from_utf8(e.name())?.to_string());
                        }
                        ()
                    }
                }
            },
            Ok(Event::End(ref e)) => {
                match e.name() {
                    b"binary" | b"description" => { ws.skip = false; }
                    b"p" => {
                        ws.line_done();
                        if !ws.in_title && ws.first_paragraph {
                            ws.first_paragraph = false;
                        }
                    }
                    b"v" => {
                        ws.line_done();
                    }
                    b"poem" => {
                        ws.ensure_empty_line();
                    }
                    b"stanza" => {
                        ws.ensure_empty_line();
                    }
                    b"section" => {
                        // FIXME do this only for the "outer" sections.
                        ws.ensure_empty_line();
                        // TODO Here the decoration is prefixed with some position
                        // in the book, which is incorrect.
                        ws.align = Align::Center;
                        ws.push_word("✦ ✦ ✦");
                        ws.line_done();
                        ws.push_empty_line();
                        ws.align = Align::Left;
                    }
                    b"epigraph" => {
                        ws.change_prefix("");
                        ws.ensure_empty_line();
                        ws.align = Align::Left;
                    }
                    b"cite" => {
                        ws.change_prefix("");
                        ws.ensure_empty_line();
                    }
                    b"emphasis" => {
                        ws.push_fmt_end(FBstyle::Emph);
                    }
                    b"strong" => {
                        ws.push_fmt_end(FBstyle::Strong);
                    }
                    b"title" => {
                        ws.push_fmt_end(FBstyle::Title);
                        ws.ensure_empty_line();
                        ws.in_title = false;
                        ws.first_paragraph = true;
                    }
                    b"subtitle" => {
                        ws.push_fmt_end(FBstyle::Subtitle);
                        ws.ensure_empty_line();
                        ws.first_paragraph = true;
                        //ws.in_title = false;
                    }
                    b"text-author" => {
                        ws.line_done();
                        ws.align = Align::Left;
                    }
                    _ => (),
                }
            },

            Ok(Event::Text(e)) => {
                if !ws.skip {
                  let t = e.unescape_and_decode(&reader)?;
                  ws.xml_offset.tag_count += 1;
                  ws.xml_offset.word_offset = 0;
                  hyphenator.out (&t, ws)?;
                }
            },
            Ok(Event::Empty(e)) => {
                match e.name() {
                    b"empty-line" => {
                        ws.push_empty_line();
                    }
                    _ => {
                        if !ws.skip {
                            ws.tags.insert(
                                std::str::from_utf8(e.name())?.to_string());
                        }
                    }
                }
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Error at position {}: {:?}",
                           reader.buffer_position(), e))
            }

            Ok(Event::Eof) => {
                ws.line_done();
                ws.eof = true;
                break
            },
            _ => (),
        }

    }
    Ok(())
}

// fill the screen starting from the line at index `line_idx`,
// and assuming that the screen is `height` lines.
fn print_n_lines (ws : &mut WriterState,
                  start_idx : usize,
                  lines : usize) -> usize {

    let mut i = 0;
    while start_idx + i < ws.lines.len() && i < lines {
        let l = &ws.lines[start_idx+i];
        print!("{:<4}{}\r\n", " ", l.content);
        // XXX this is only for debugging, we will get rid of xml offsets.
        /* if let Some(o) = l.xml_offset {
            print!("{:<4}{:<4}    {}\r\n", o.tag_count,
                                           o.word_offset, l.content);
        }
        else {
            print!("{:<8}    {}\r\n", "---", l.content);
        } */
        i += 1;
    }
    i
}

fn main () -> anyhow::Result<()> {
    let app = app_from_crate!()
             .arg(
                Arg::with_name("input")
                    .help("input file containing the fb2 book")
                    .index(1)
                    .required(true),
              )
              .get_matches();

    // TODO add a flag that can specify where the settings live,
    // and use some default location, using xdg defaults.
    //
    // Read the config file for the termbook, including
    // the state of the books that we have ever read.
    let config_fname = "settings.yml";
    let config_file_r = std::fs::File::open(config_fname)
        .with_context(|| format!("cannot open settings `{}'", config_fname))?;
    let mut tbconf: TBconfig
        = serde_yaml::from_reader(std::io::BufReader::new(config_file_r))?;

    // The location of the book that we are about to open.
    let input = app.value_of("input").ok_or(ProcessingError::new(
            &"cannot get the value of the input file"))?;

    // Get absolute path of the book --- we use it as a key in the file
    // that keeps states (tag_offset and word offset).
    let input_rel = std::path::PathBuf::from(input);
    let input_abs = std::fs::canonicalize(&input_rel)?
                    // TODO get rid of this unwrap
                    .into_os_string().into_string().unwrap();

    let f = std::fs::File::open(&input)
            .with_context(|| format!("cannot open file `{}'", input))?;

    let mut za;
    let mut reader : Reader<Box<dyn BufRead>> =
    // If we have a zipped file, we'd have to unzip it first.
    if input_rel.extension() == Some(std::ffi::OsStr::new("zip")) {
        za = zip::read::ZipArchive::new(f)?;
        let zf = za.by_index(0)?;
        let zfr = std::io::BufReader::new(zf);
        quick_xml::Reader::from_reader(Box::new(zfr))
    } else {
        let fr = std::io::BufReader::new(f);
        quick_xml::Reader::from_reader(Box::new(fr))
    };

    // TODO: parse <description> of the book and choose the appropriate
    // language, and possible get other meta-information.
    let hyphenator = Standard::from_embedded(Language::Russian)?;

    // get terminal size
    //
    // FIXME in some cases when the terminal is ridiculously
    // small we have to give an error message or simply dump
    // the text without much formatting.
    let (w16, h16) = terminal_size()?;
    // Convert the u16 size into usize.
    let w = w16 as usize;
    let h = h16 as usize;

    // Prepare the state structure for the xml parser.
    let lines = Vec::new();
    let l = String::from("");
    let styles = Vec::new();
    let tags = std::collections::HashSet::<String>::new();

    // TODO read this from the config file.
    let mut smap = std::collections::HashMap::new();
    smap.insert(FBstyle::Strong,(style::Bold.to_string(), style::NoBold.to_string()));
    smap.insert(FBstyle::Title,(color::Fg(color::LightBlue).to_string(),
                                color::Fg(color::Reset).to_string()));

    smap.insert(FBstyle::Subtitle,(color::Fg(color::LightBlue).to_string(),
                                color::Fg(color::Reset).to_string()));
    smap.insert(FBstyle::Emph,(color::Fg(color::LightCyan).to_string(),
                                color::Fg(color::Reset).to_string()));

    smap.insert(FBstyle::Bold,(color::Fg(color::LightGreen).to_string(),
                                color::Fg(color::Reset).to_string()));

    assert!(w>12);
    let mut ws = WriterState { line: 0, pos: 0,
                               // TODO use config to set maxline.
                               line_width: core::cmp::min((w-12).into(),50),
                               l: l,
                               lines: lines,
                               eof: false,
                               xml_offset: BookState{tag_count:0, word_offset:0},
                               tags: tags,
                               prefix: String::from(""), needs_prefix: true,
                               align: Align::Left,
                               smap: smap,
                               styles: styles,
                               in_title: false, skip: false,
                               last_line_empty: false,
                               first_paragraph: true};


    // Prepare to start termion with terminal in raw mode.
    let stdin = stdin();
    let mut stdout = stdout().into_raw_mode()?;

    // So far this is our index into the ws.lines which we use
    // when KeyDown is pressed, so that we know how much lines
    // do we print.
    let mut lines_idx = 0;

    write!(stdout,
           "{}{}{}",
           termion::clear::All,
           termion::cursor::Goto(1, 1),
           termion::cursor::Hide)?;
    stdout.flush()?;

    // check whether we have a saved position of that book in
    // the config file.
    if tbconf.books.contains_key(&input_abs) {
        let bstate = tbconf.books.get(&input_abs).ok_or(ProcessingError::new(
                &format!("error obtaining a state of `{}'", &input_abs)))?;
        // read enough text
        while !ws.eof
              // Automatic lexicographic order due to ParialOrd.
              && ws.xml_offset < *bstate {
            crank(&mut reader, &hyphenator, &mut ws, 100)?;
        }
        // find the index of the line that is "closest" to the
        // saved state.
        //    - If we don't find the offset that is smaller
        //      than the stored one, we start from the beginning of the book.
        //    - If the offset is too large (bogus config file) we'll end-up
        //      at the last line of the book.
        lines_idx = ws.lines.iter()
                    .rposition(|p| match p.xml_offset {
                                      Some(o) => o <= *bstate,
                                      None => false
                                    })
                    .unwrap_or(0);
    }

    // print the initial screen of text.
    // TODO lift this validation up.
    assert!(h>1);
    crank(&mut reader, &hyphenator, &mut ws, h)?;
    lines_idx += print_n_lines(&mut ws, lines_idx, h-1);
    stdout.flush()?;

    for c in stdin.keys() {
        match c.unwrap() {
            Key::Char('q') => {
                // Grab the first non-empty offset, or (0,0) in case we don't have any.
                let mut i = lines_idx.saturating_sub(h-1);
                let mut s = BookState { tag_count: 0, word_offset: 0};
                while i > 0 {
                    match ws.lines[i].xml_offset {
                        Some(o) => {
                            s = o;
                            break;
                        }
                        None => i -= 1
                    }
                }
                // add or update the book position.
                tbconf.books.insert(input_abs, s);

                // save the config into the yaml file.
                let config_file_w = std::fs::File::create(config_fname)?;
                serde_yaml::to_writer(std::io::BufWriter::new(&config_file_w), &tbconf)?;
                config_file_w.sync_all()?;
                break
            }
            Key::Up => {
                termion::cursor::Goto(1,1);
                lines_idx = lines_idx.saturating_sub(h);
                lines_idx += print_n_lines(&mut ws, lines_idx, h-1)
            }
            Key::PageUp => {
                termion::cursor::Goto(1,1);
                lines_idx = lines_idx.saturating_sub(2*h-2);
                lines_idx += print_n_lines(&mut ws, lines_idx, h-1)
            }
            Key::Down => {
                if lines_idx+1 >= ws.lines.len() {
                  // XXX here 10 is just a magic number...
                  crank(&mut reader, &hyphenator, &mut ws, 10)?;
                }
                lines_idx += print_n_lines(&mut ws, lines_idx, 1);
            }
            Key::PageDown => {
                if lines_idx+(h as usize) >= ws.lines.len() {
                  crank(&mut reader, &hyphenator, &mut ws, h)?;
                }
                lines_idx += print_n_lines(&mut ws, lines_idx, h-1);
            }
            _ => {}
        }
        stdout.flush()?;
    }

    write!(stdout, "{}", termion::cursor::Show)?;
    // XXX this is debugging info.
    for x in &ws.tags {
        print!("{}\r\n", x);
    }
    Ok(())
}
