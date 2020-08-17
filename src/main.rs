//extern crate hyphenation;
//extern crate textwrap;

use hyphenation::{Hyphenator,Language, Load, Standard};
//use textwrap::Wrapper;
use quick_xml::{Reader};
use quick_xml::events::Event;
//use ansi_term::{Style};

use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::{style,color,terminal_size};
use std::io::{BufRead, Write, stdout, stdin};
use std::mem;

use clap::{Arg,App,app_from_crate,
           crate_name,crate_version,crate_authors,crate_description};

use anyhow::{Context,Result};
//use std::io::BufReader;
//use std::fs::File;

//#[derive(PartialEq, Clone, Copy)]
struct WriterState {
    pub line : usize,
    pub pos: usize,
    pub line_width: usize,
    pub l: String,
    // We keep the buffer that we have processed so far
    // and the xml offsets for each line, for a more precise
    // reloading.  XXX put them together in one Vec?
    pub lines: Vec<String>,
    pub lines_xml_offsets: Vec<(usize, usize)>,
    pub eof: bool,
    // We use these fields to annotate the lines with their positions
    // in the xml document, so that we could restore it on the next load.
    pub xml_txt_count : usize,  // count text tags in the xml stream
    pub xml_txt_off : usize,    // offset within the text
}

impl WriterState {
    fn line_done(&mut self) {
        let t = mem::replace(&mut self.l, String::from(""));
        self.lines.push(t);
        self.pos = 0;
        self.lines_xml_offsets.push((self.xml_txt_count, self.xml_txt_off));
    }
    fn dprint(&self) {
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
                //print!("{}{}", space,  w);
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
                            let t = l.iter().collect::<String>();
                            // FIXME refactor to use line_done here.
                            state.lines.push(t);
                            // FIXME here the information about the xml position
                            // of the line is quite imprecise, as the word is too
                            // long.  We may deal with this by making the xml_pos
                            // structure a bit more precise.
                            state.lines_xml_offsets.push((state.xml_txt_count,
                                                          state.xml_txt_off));   
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
                        //(for now) ws.lines.push(String::from(""));
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
        let xo = ws.lines_xml_offsets[start_idx+i];
        print!("{:<4}{:<4}    {}\r\n", xo.0, xo.1, ws.lines[start_idx+i]);
        i += 1;
    }
    let wrote = i;
    /* while i < lines {
        print!("    ~\r\n");
        i += 1;
    } */
    return wrote;
}



fn main () -> Result<()> {
    let app = app_from_crate!()
             .arg(
                Arg::with_name("input")
                    .help("input file containing the fb2 book")
                    .index(1)
                    .required(true),
              )
              .get_matches();


    let input = app.value_of("input").unwrap();
    let mut reader = Reader::from_file(input)
                     .with_context(|| format!("cannot open file `{}'", input))?;
    
    let hyphenator = Standard::from_embedded(Language::Russian)?;

    // get terminal size
    let (w, h) = terminal_size()?;
    // FIXME in some cases when the terminal is ridiculously
    // small we have to give an error message or simply dump
    // the text without much formatting.

    let lines = Vec::new();
    let lines_xml_offsets = Vec::new();
    let l = String::from("");
    let mut ws = WriterState { line: 0, pos: 0, 
                               line_width: core::cmp::min((w-8).into(),80),
                               l: l, 
                               lines: lines,
                               lines_xml_offsets: lines_xml_offsets,
                               eof: false,
                               xml_txt_count: 0,
                               xml_txt_off: 0};
    //let mut loopc = 0;


    // Here starts the termion example
    let stdin = stdin();
    let mut stdout = stdout().into_raw_mode()?;

    // So far this is our index into the ws.lines which we use
    // when KeyDown is pressed, so that we know how much lines
    // do we print.
    let mut lines_idx = 0;

    write!(stdout,
           "{}{}{}", //q to exit. Type stuff, use alt, and so on.{}",
           termion::clear::All,
           termion::cursor::Goto(1, 1),
           termion::cursor::Hide)
           .unwrap();
    stdout.flush()?;

    //print!("teminal size is: {} x {}\r\n", w, h);
    // print the initial screen of text.
    crank(&mut reader, &hyphenator, &mut ws, h.into());
    lines_idx += print_n_lines(&mut ws, lines_idx, (h-1).into());
    stdout.flush()?;

    // fill the first screen of the terminal with the text
    //fill_screen(&mut reader, &hyphenator, &mut ws, lines_idx, h);


    for c in stdin.keys() {
        // write!(stdout,
        //        "{}{}",
        //        termion::cursor::Goto(1, 1),
        //        termion::clear::CurrentLine)
        //         .unwrap();

        match c.unwrap() {
            Key::Char('q') => break,
            Key::Char(c) => println!("{}", c),
            Key::Alt(c) => println!("^{}", c),
            Key::Ctrl(c) => println!("*{}", c),
            Key::Esc => println!("ESC"),
            Key::Left => println!("←"),
            Key::Right => println!("→"),
            Key::Up => {
                //println!("↑");
                //termion::scroll::Up(5);
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
                //println!("↓"),
            Key::Backspace => println!("×"),
            _ => {}
        }
        stdout.flush().unwrap();
    }

    write!(stdout, "{}", termion::cursor::Show)?;
    Ok(())
    /*
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => {
                match e.name() {
                    b"p" => { 
                        //ws = hyphenator.out (&"  ", ws);
                        let s = "    ";
                        print!("{}", s);
                        ws.pos = s.len();
                    },
                    b"emphasis" => {
                        print!("{}", st_bold.prefix());
                    },
                    _ => (),
                }
            },
            Ok(Event::End(ref e)) => {
                match e.name() {
                    b"p" => { 
                        //in_p = false;
                        println!("");

                    }
                    b"emphasis" => { 
                        //in_emph = false; 
                        print!("{}", st_bold.suffix());
                    }
                    _ => (),
                }
            },

            Ok(Event::Text(e)) => {
                loopc += 1;
                let t = e.unescape_and_decode(&reader).unwrap();
                ws = hyphenator.out (&t, ws);
                // if in_emph {
                //     let emph_t = Style::new().bold().paint(t).to_string();
                //     p.push_str (&emph_t);
                // } else {
                //     p.push_str (&t);
                // }
                if loopc > 200 {

                    //break;
                }
            },
            Err(e) => panic!("Error at position {}: {:?}", reader.buffer_position(), e),
            Ok(Event::Eof) => break,
            _ => (),
        }
    }
    buf.clear();
    */


    // FIXME use textwrap::termwidth() for the width of the terminal
    // let wrapper = Wrapper::with_splitter(80, hyphenator);
    // 
    // let mut count = 0;
    // for t in &txt {
    //     count = count + 1;
    //     //println!("count: {}", count);
    //     if count < 12 {
    //         println!("{}|\n", t); // wrapper.fill(t));
    //     }
    // }

    //let text = "textwrap: a small library for wrapping text.";
    //println!("{}", wrapper.fill(text))
}
