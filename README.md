# Terminal Book Reader

This is an application for reading `.fb2` books in terminal.

Disclaimer: the main purpose of this project is me learning `Rust`; therefore,
please do not expect a production ready application.

## Features
Nevertheless, the terminal reader is capable of showing the content of `.fb2`
books providing:
  - hyphenation (using the
    [hyphenation rust library](https://github.com/tapeinosyne/hyphenation))
  - scrolling
  - save/restore book position (even if the terminal size changes)

## Missing features
Missing features that I would like to add:
  - more fancy formatting:
    * support all the `fb2` tags;
    * find a nice style for the elements (underline, utf-8 symbols, ...)
  - hanging syllables when hyphenating: right now I am using a greedy algorithm
    which doesn't always produce beautiful results.
    I would like to implement something in the style of
    [`par` utility](https://en.wikipedia.org/wiki/Par_(command)) so that
    the left side of the text becomes pretty.
  - proper config file: right now I am using config file to read/write
    positions of the books we read, no settings like
    colors, default widths, or other options.
  - navigate using chapters

## Features I am not sure about
Features with a low priority:
  - searching: I almost never find myself in the position that I'd like to
    search something while reading a fiction book; so I am not sure this is
    really needed.
  - command line: while this is fun to implement, I am not sure what kind of
    commands does the book reader really need.
  - table of contents: maybe.

## Development
Please feel free to propose a feature, or better a patch or report a bug.
While my progress on this project is very slow, it does move forward.
