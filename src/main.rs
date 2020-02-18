//extern crate hyphenation;
//extern crate textwrap;

use hyphenation::{Hyphenator,Language, Load, Standard};
//use textwrap::Wrapper;
use quick_xml::Reader;
use quick_xml::events::Event;
use ansi_term::{Style};


//use std::io::BufReader;
//use std::fs::File;

#[derive(PartialEq, Clone, Copy)]
struct WriterState {
    pub pos: usize,
    pub line_width: usize,
}


trait OutText {
    fn out (&self, s: &str, state: WriterState) -> WriterState;
}

impl OutText for Standard {
    fn out (&self, s: &str, state: WriterState) -> WriterState {
        let mut chars_left = state.line_width - state.pos;

        // FIXME Wrirte style begin
        
        
        // FIXME this is a hack, as we need to check that the last unicode
        // character is a whitespace kind of thing.
        let last_space = s.ends_with(" ");

        for (i, w) in s.split_whitespace().enumerate() {
            let wlen = w.chars().count();
            if wlen + 1 < chars_left {
                let space = if i == 0 { "" } else { " " };
                print!("{}{}", space,  w);
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
                        println!(" {}{}", head, hyp);
                        // FIXME what if the length of the tail > line_widht?
                        print!("{}", tail);
                        chars_left = state.line_width - tail.chars().count();
                        hyp_found = true;
                        break;
                    }
                }
    
                // If we didn't find the hyphenation, break right here
                if !hyp_found {
                    println!("");
                    // If `w` is crazily long, we'll just break in the middle
                    if wlen > state.line_width {
                        let v: Vec<_> = w.chars().collect();
                        for l in v.chunks (state.line_width) {
                            println!("{}", l.iter().collect::<String>());
                            chars_left = state.line_width - l.len();
                        }
                        //panic!("word {} too long!", w);
                    } else {
                        print!("{}", w);
                        chars_left = state.line_width - wlen;
                    }
                }

            }
        }
        
        if last_space {
            // FIXME check that we are not negative.
            print!(" ");
            chars_left -= 1;
        }
        
        // FIXME Write style end
        WriterState { pos: state.line_width - chars_left, line_width: state.line_width }
    }
}


fn main () {
    // Error handling for file reading later.
    //let f1 = match File::open("x.fb2") {
    //    Err(e) => panic!("Error {}", e),
    //    Ok(f) => f
    //};
    //let reader = BufReader::new(f1);

    let mut reader = Reader::from_file("x.fb2").unwrap();
    //reader.trim_text(true);
    let mut buf = Vec::new();
    //let mut txt = Vec::new();
    
    let hyphenator = Standard::from_embedded(Language::Russian).unwrap();



    // FIXME Create a struct with fields
    //let mut in_p = false;
    //let mut in_emph = false;
    //let mut p = "".to_owned();

    let mut ws = WriterState { pos: 0, line_width: 80 };
    let mut loopc = 0;

    let st_bold = Style::new().bold();

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
