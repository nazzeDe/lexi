use std::io::{self, Write};

pub fn write_record(
    writer: &mut impl Write,
    dictionary_name: &str,
    headword: &str,
    definition: &str,
    show_dictionary: bool,
    first: bool,
) -> io::Result<()> {
    if !first {
        writeln!(writer)?;
    }
    if show_dictionary {
        writeln!(writer, "[{dictionary_name}] {headword}")?;
    } else {
        writeln!(writer, "{headword}")?;
    }
    writer.write_all(definition.as_bytes())?;
    if !definition.ends_with('\n') {
        writeln!(writer)?;
    }
    Ok(())
}

pub fn write_miss(writer: &mut impl Write, term: &str) -> io::Result<()> {
    writeln!(writer, "No entry found for: {term}")
}

#[cfg(test)]
mod tests {
    use super::{write_miss, write_record};

    #[test]
    fn pads_definition_newline_and_separates_records() {
        let mut buf = Vec::new();
        write_record(&mut buf, "oxford", "a", "one", false, true).unwrap();
        write_record(&mut buf, "oxford", "b", "two\n", false, false).unwrap();
        assert_eq!(buf, b"a\none\n\nb\ntwo\n");
    }

    #[test]
    fn show_dictionary_uses_bracket_title_and_verbatim_definition() {
        let mut buf = Vec::new();
        write_record(
            &mut buf,
            "Oxford",
            "hello",
            "  keep \nand a blank\n",
            true,
            true,
        )
        .unwrap();
        assert_eq!(buf, b"[Oxford] hello\n  keep \nand a blank\n");
    }

    #[test]
    fn miss_uses_the_contract_prefix() {
        let mut buf = Vec::new();
        write_miss(&mut buf, "helo").unwrap();
        assert_eq!(buf, b"No entry found for: helo\n");
    }
}
