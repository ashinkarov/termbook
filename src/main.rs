use hyphenation::{Hyphenator,Language, Load, Standard};
use quick_xml::{Reader};
use quick_xml::events::Event;

use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::{style,color,terminal_size};

use std::io::{BufRead, Write, stdout, stdin};
use std::mem;

use clap::{Arg,app_from_crate,
           crate_name,crate_version,crate_authors,crate_description};

use anyhow::{Context};

use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
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
struct WriterState {
    pub line : usize,
    pub pos: usize,
    pub line_width: usize,
    pub l: String,
    pub lines: Vec<Line>,
    pub eof: bool,
    // We use these fields to annotate the lines with their positions
    // in the xml document, so that we could restore it on the next load.
    // XXX we can use BookState here as well.
    pub xml_txt_count : usize,  // count text tags in the xml stream
    pub xml_txt_off : usize,    // offset within the text
}

impl WriterState {
    fn line_done(&mut self) {
        let t = mem::replace(&mut self.l, String::from(""));
        let o = BookState { tag_count: self.xml_txt_count,
                            word_offset: self.xml_txt_off };

        self.lines.push(Line {xml_offset: Some(o), content: t});
        self.pos = 0;
    }
    fn _dprint(&self) {
        print!("line: {}, pos: {}, eof: {}", self.line, self.pos, self.eof);
    }
}


trait OutText {
    fn out (&self, s: &str, state: &mut WriterState) -> (); // WriterState;
}

impl OutText for Standard {
    fn out (&self, s: &str, state: &mut WriterState) -> () { //WriterState {
        let mut chars_left = state.line_width - state.pos;
        let mut line = state.line;

        // FIXME this is a hack, as we need to check that the last unicode
        // character is a whitespace kind of thing.
        let last_space = s.ends_with(" ");

        for (i, w) in s.split_whitespace().enumerate() {
            let wlen = w.chars().count();
            if wlen + 1 < chars_left {
                let space = if i == 0 { "" } else { " " };
                state.l.push_str (space);
                state.l.push_str (w);
                chars_left -= wlen + space.len();
            } else {
                // Hyphenate the word
                let mut triples = Vec::new();
                for n in self.hyphenate(w).breaks {
                    let (head, tail) = w.split_at(n);
                    let hyphen = if head.ends_with('-') { "" } else { "-" };
                    triples.push((head, hyphen, tail));
                }

                // Now iterate the tripletes
                let mut hyp_found = false;
                for &(head, hyp, tail) in triples.iter().rev() {
                    let prefix = head.chars().count() + hyp.chars().count();
                    if prefix + 1 <= chars_left {
                        //print!(" {}{}\r\n", head, hyp);
                        // FIXME what if the length of the tail > line_widht?
                        //print!("{}", tail);
                        state.l.push_str (" ");
                        state.l.push_str (head);
                        state.l.push_str (hyp);
                        state.line_done();
                        //let t = mem::replace(&mut state.l, String::from(""));
                        //state.lines.push(t);
                        state.l.push_str (tail);

                        // update xml_txt_off with the current word count `i`
                        state.xml_txt_off = i;

                        chars_left = state.line_width - tail.chars().count();
                        hyp_found = true;
                        line += 1;
                        break;
                    }
                }

                // If we didn't find the hyphenation, break right here
                if !hyp_found {
                    state.line_done();
                    // update xml_txt_off with the current word count `i`
                    state.xml_txt_off = i;

                    line += 1;
                    // If `w` is crazily long, we'll just break in the middle
                    if wlen > state.line_width {
                        // FIXME this is quite weird now, the last chunk of
                        // the `w` might be shorter than the line...
                        let v: Vec<_> = w.chars().collect();
                        for l in v.chunks (state.line_width) {
                            state.l = l.iter().collect::<String>();
                            state.line_done();
                            chars_left = state.line_width - l.len();
                            line += 1;
                        }
                    } else {
                        //print!("{}", w);
                        state.l.push_str (w);
                        chars_left = state.line_width - wlen;
                    }
                }

            }
        }

        if last_space {
            // FIXME check that we are not negative.
            //print!(" ");
            state.l.push_str (" ");
            chars_left -= 1;
        }

        state.line = line;
        state.pos = state.line_width - chars_left;

    }
}


fn crank<B: BufRead> (reader : &mut Reader<B>,
                      hyphenator: &Standard,
                      ws : &mut WriterState,
                      // how many lines do we accumulate
                      count : usize) {

    let mut buf = Vec::new();
    //let st_bold = Style::new().bold();
    let st_bold = style::Bold;
    let st_nobold = style::NoBold;
    let c_title = color::Fg(color::LightBlue);
    let c_reset = color::Fg(color::Reset);

    // XXX this is a hack to skip the text contained in tags that
    // we don't care about.  We set the skip flag on the beginning
    // of the tag, and unset it at the end.
    let mut skip = false;

    let l = ws.line + count;
    while !ws.eof && ws.line < l {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => {
                match e.name() {
                    b"description" => { skip = true; }
                    b"p" => {
                        // FIXME this looks weird, don't we need to make
                        // the line in the hyphenator shorter?
                        let s = "    ";
                        ws.l.push_str(s);
                        ws.pos = s.len();
                    },
                    b"emphasis" => {
                        ws.l.push_str (&st_bold.to_string());
                    },
                    b"title" => {
                        ws.lines.push(Line {xml_offset: None, content: String::from("")});
                        ws.l.push_str (&c_title.to_string());
                    }
                    _ => (),
                }
            },
            Ok(Event::End(ref e)) => {
                match e.name() {
                    b"description" => { skip = false; }
                    b"p" => {
                        ws.line_done();
                        //(for now) ws.lines.push(String::from(""));

                    }
                    b"emphasis" => {
                        ws.l.push_str (&st_nobold.to_string());
                    }
                    b"title" => {
                        //(for now) ws.lines.push(String::from(""));
                        ws.l.push_str (&c_reset.to_string());
                    }
                    _ => (),
                }
            },

            Ok(Event::Text(e)) => {
                if !skip {
                  let t = e.unescape_and_decode(&reader).unwrap();
                  ws.xml_txt_count += 1;
                  ws.xml_txt_off = 0;
                  hyphenator.out (&t, ws);
                }
            },
            Err(e) => panic!("Error at position {}: {:?}",
                                reader.buffer_position(), e),
            Ok(Event::Eof) => {
                ws.line_done();
                ws.eof = true;
                break
            },
            _ => (),
        }

    }
}

// fill the screen starting from the line at index `line_idx`,
// and assuming that the screen is `height` lines.
fn print_n_lines (ws : &mut WriterState,
                  start_idx : usize,
                  lines : usize) -> usize {

    let mut i = 0;
    while start_idx + i < ws.lines.len() && i < lines {
        // XXX this is only for debugging, we will get rid of xml offsets.
        let l = &ws.lines[start_idx+i];
        if let Some(o) = l.xml_offset {
            print!("{:<4}{:<4}    {}\r\n", o.tag_count,
                                           o.word_offset, l.content);
        }
        else {
            //let xo = ws.lines_xml_offsets[start_idx+i];
            print!("{:<8}    {}\r\n", "---", l.content);
        }
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
    let config_file = std::fs::File::open(config_fname)
        .with_context(|| format!("cannot open settings `{}'", config_fname))?;
    let mut tbconf: TBconfig
        = serde_yaml::from_reader(std::io::BufReader::new(config_file))?;

    // The location of the book that we are about to open.
    let input = app.value_of("input").unwrap();
    let mut reader = Reader::from_file(input)
                     .with_context(|| format!("cannot open file `{}'", input))?;

    // Get absolute path of the book --- we use it as a key in the file
    // that keeps states (tag_offset and word offset).
    let input_rel = std::path::PathBuf::from(input);
    let input_abs = std::fs::canonicalize(&input_rel).unwrap()
                    .into_os_string().into_string().unwrap();


    // TODO: parse <description> of the book and choose the appropriate
    // language, and possible get other meta-information.
    let hyphenator = Standard::from_embedded(Language::Russian)?;

    // get terminal size
    let (w, h) = terminal_size()?;
    // FIXME in some cases when the terminal is ridiculously
    // small we have to give an error message or simply dump
    // the text without much formatting.

    // Prepare the state structure for the xml parser.
    let lines = Vec::new();
    let l = String::from("");
    let mut ws = WriterState { line: 0, pos: 0,
                               line_width: core::cmp::min((w-12).into(),80),
                               l: l,
                               lines: lines,
                               eof: false,
                               xml_txt_count: 0,
                               xml_txt_off: 0};


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
           termion::cursor::Hide)
           .unwrap();
    stdout.flush()?;

    // check whether we have a saved position of that book in
    // the config file.
    if tbconf.books.contains_key(&input_abs) {
        // TODO add validation of the stored data, e.g. if the offset
        // in the config file is completely bogus.
        let bstate = tbconf.books.get(&input_abs).unwrap();
        // read enough text
        while !ws.eof
              // TODO abstract into lexicographical comparison
              && (ws.xml_txt_count < bstate.tag_count
                  || ws.xml_txt_count == bstate.tag_count
                     &&  ws.xml_txt_off < bstate.word_offset) {
            crank(&mut reader, &hyphenator, &mut ws, 100);
        }
        // find the index of the line that is "closest" to the
        // saved state.
        lines_idx = ws.lines.iter()
                    .rposition(|p| match p.xml_offset {
                                      Some(o) => o.tag_count <= bstate.tag_count
                                                 && o.word_offset <= bstate.word_offset,
                                      None => false
                                    })
                    .unwrap_or(0);
    }

    // print the initial screen of text.
    crank(&mut reader, &hyphenator, &mut ws, h.into());
    lines_idx += print_n_lines(&mut ws, lines_idx, (h-1).into());
    stdout.flush()?;

    for c in stdin.keys() {
        match c.unwrap() {
            Key::Char('q') => {
                // offset of the first visible line on the screen.
                assert!(h>=1);
                //let s = ws.lines[lines_idx.saturating_sub((h-1) as usize)].xml_offset;
                // Grab first non-empty offset, or (0,0) in case we don't have any.
                let mut i = lines_idx.saturating_sub((h-1) as usize);
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
                let config_file = std::fs::OpenOptions::new()
                                  .write(true).open(config_fname)?;
                serde_yaml::to_writer(std::io::BufWriter::new(&config_file), &tbconf)?;
                config_file.sync_all()?;
                break
            }
            /*Key::Char(c) => println!("{}", c),
            Key::Alt(c) => println!("^{}", c),
            Key::Ctrl(c) => println!("*{}", c),
            Key::Esc => println!("ESC"),
            Key::Left => println!("←"),
            Key::Right => println!("→"),*/
            Key::Up => {
                termion::cursor::Goto(1,1);
                lines_idx = lines_idx.saturating_sub(h as usize);
                lines_idx += print_n_lines(&mut ws, lines_idx, (h-1).into())
            }
            Key::Down => {
                //println!("the length of ws.lines = {}\r\n", ws.lines.len());
                //if ws.eof {
                //    continue;
                //}
                if lines_idx+1 >= ws.lines.len() {
                  crank(&mut reader, &hyphenator, &mut ws, 10);
                }
                lines_idx += print_n_lines(&mut ws, lines_idx, 1);
                //ws.dprint(); print!("\r");
            }
            //Key::Backspace => println!("×"),
            _ => {}
        }
        stdout.flush().unwrap();
    }

    write!(stdout, "{}", termion::cursor::Show)?;
    Ok(())
}
