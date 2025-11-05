#[derive(Debug)]
pub struct QuoteFilter<'a> {
    pub quoted: Vec<&'a str>,
    pub unquoted: Vec<&'a str>,
}

impl<'a> QuoteFilter<'a> {
    pub fn from(input: &'a str) -> Self {
        const EMPTY: (&str, &str) = ("", "");

        let mut quoted: Vec<&str> = Vec::new();
        let mut unquoted: Vec<&str> = Vec::new();

        let mut first_split = input.split_once("\"");
        let mut second_split = first_split.unwrap_or(EMPTY).1.split_once("\"");
        let mut left_overs = "";

        if first_split == None {
            left_overs = input;
        }

        while first_split != None && second_split != None {
            Self::push_if_not_empty(&mut unquoted, first_split.unwrap_or(EMPTY).0.trim());
            Self::push_if_not_empty(&mut quoted, second_split.unwrap_or(EMPTY).0.trim());
            first_split = (second_split).unwrap_or(EMPTY).1.split_once("\"");
            left_overs = second_split.unwrap_or(EMPTY).1;
            second_split = (first_split).unwrap_or(EMPTY).1.split_once("\"");
        }

        Self::push_if_not_empty(&mut unquoted, left_overs.trim());

        QuoteFilter { quoted, unquoted }
    }

    fn push_if_not_empty<'b>(input: &mut Vec<&'b str>, value: &'b str) {
        if !value.is_empty() {
            input.push(value);
        }
    }
}
