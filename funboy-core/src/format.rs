use std::{borrow::Cow, str::FromStr};

use crate::{FunboyError, database::Identifiable};

pub const TWO_THOUSAND: usize = 2000;
pub const ONE_HUNDRED: usize = 100;

pub fn parse_bot_args(input: &str) -> Result<Vec<&str>, FunboyError> {
    let input = input.trim();

    let mut output: Vec<&str> = vec![];
    let mut inside_quotes = false;
    let mut prev_ch_space = false;
    let mut prev_ch_quote = false;
    let mut word_start: usize = 0;

    for (i, ch) in input.char_indices() {
        if ch == '"' {
            if !inside_quotes {
                if !prev_ch_space && !prev_ch_quote && i > 0 {
                    output.push(&input[word_start..i]);
                }
                word_start = i + ch.len_utf8();
            } else {
                output.push(&input[word_start..i]);
                word_start = i + 1;
            }
            inside_quotes = !inside_quotes;
        } else if !ch.is_whitespace() && prev_ch_space && !inside_quotes {
            word_start = i;
        } else if ch.is_whitespace() && !prev_ch_space && !inside_quotes && !prev_ch_quote {
            output.push(&input[word_start..i]);
            word_start = i + 1;
        }

        prev_ch_quote = ch == '"';
        prev_ch_space = ch.is_whitespace();
    }

    if inside_quotes {
        return Err(FunboyError::UserInput(
            "unclosed quote in substitutes".into(),
        ));
    }

    if word_start < input.len() {
        output.push(&input[word_start..]);
    }

    Ok(output)
}

pub trait AsStrs {
    fn as_strs(&self) -> Vec<&str>;
}

impl AsStrs for Vec<String> {
    fn as_strs(&self) -> Vec<&str> {
        self.iter().map(String::as_ref).collect()
    }
}

pub fn split_block<'a>(str: &'a str, max_message_size: usize) -> Vec<&'a str> {
    let mut output = Vec::new();
    let blocks: usize = str.len() / max_message_size;

    for i in 0..blocks {
        output.push(&str[i * max_message_size..(i + 1) * max_message_size]);
    }

    if blocks * max_message_size < str.len() {
        output.push(&str[blocks * max_message_size..str.len()]);
    }

    output
}

pub fn split_message(input: &str, max_message_size: usize) -> Vec<&str> {
    let mut messages: Vec<&str> = vec![];
    let mut end_of_last_word: usize = 0;
    let mut end_of_last_word_prev: usize = 0;
    let mut prev_char_was_whitespace = false;
    let mut start: usize = 0;

    for (i, ch) in input.char_indices() {
        if i > 0 && ch.is_whitespace() && !prev_char_was_whitespace {
            end_of_last_word = i;
        }

        if end_of_last_word - start >= max_message_size {
            messages.push(&input[start..end_of_last_word_prev]);
            start = end_of_last_word_prev;
        }

        end_of_last_word_prev = end_of_last_word;
        prev_char_was_whitespace = ch.is_whitespace();
    }

    for block in split_block(&input[start..input.len()], max_message_size) {
        messages.push(block);
    }

    messages
}

pub fn split_messages(message: &[&str], max_message_size: usize) -> Vec<String> {
    let mut message_split: Vec<String> = Vec::new();

    let iter = message.iter();
    let mut message_part: String = String::default();

    for value in iter {
        if message_part.len() + value.len() <= max_message_size {
            message_part.push_str(value);
        } else {
            message_split.push(message_part);
            message_part = String::default();
            if value.len() <= max_message_size {
                message_part.push_str(value);
            } else {
                for sub_str in split_message(value, max_message_size) {
                    message_split.push(sub_str.to_string());
                }
            }
        }
    }

    if !message_part.is_empty() {
        message_split.push(message_part);
    }

    message_split
}

pub trait TruncateEllipsize {
    fn truncate_with_ellipse<'a>(&'_ self, new_len: usize) -> Cow<'_, str>;
}

impl TruncateEllipsize for str {
    fn truncate_with_ellipse<'a>(&'_ self, new_len: usize) -> Cow<'_, str> {
        let marcation = "...";
        let limit = new_len - marcation.len();

        if limit > self.len() {
            Cow::Borrowed(self)
        } else {
            match self.get(0..limit) {
                Some(substr) => Cow::Owned(substr.to_owned() + "..."),
                None => Cow::Borrowed(""),
            }
        }
    }
}

#[derive(Copy, Clone)]
pub struct SeperatedListOptions<'a> {
    pub item_seperator: &'a str,
    pub block_marker: &'a str,
    pub quote_multi_word_items: bool,
}

impl SeperatedListOptions<'_> {
    pub fn none() -> Self {
        Self {
            item_seperator: "",
            block_marker: "",
            quote_multi_word_items: false,
        }
    }

    pub fn space_seperated() -> Self {
        Self {
            item_seperator: " ",
            block_marker: "",
            quote_multi_word_items: true,
        }
    }

    pub fn comma_seperated() -> Self {
        Self {
            item_seperator: ", ",
            block_marker: "",
            quote_multi_word_items: true,
        }
    }
}

impl Default for SeperatedListOptions<'_> {
    fn default() -> Self {
        Self {
            item_seperator: ", ",
            block_marker: "```",
            quote_multi_word_items: true,
        }
    }
}

pub type ListFormatter = Box<dyn Fn(&[&str]) -> Vec<String> + Send + Sync>;

pub fn format_as_item_seperated_list(
    items: &[&str],
    caption: &str,
    options: SeperatedListOptions,
) -> Vec<String> {
    let mut messages: Vec<String> = Vec::new();
    messages.push(String::with_capacity(TWO_THOUSAND));
    let mut current_msg = 0;

    messages[current_msg].push_str(options.block_marker);
    for (i, item) in items.iter().enumerate() {
        let item = if options.quote_multi_word_items && item.contains(char::is_whitespace) {
            format!("\"{}\"", item)
        } else {
            format!("{}", item)
        };

        let item = if item.len()
            > TWO_THOUSAND
                - (options.block_marker.len() * 2)
                - caption.len()
                - options.item_seperator.len()
        {
            format!("{}", &item.truncate_with_ellipse(ONE_HUNDRED))
        } else {
            item
        };

        let addition_len = messages[current_msg].len() + item.len() + options.block_marker.len();

        let seperator = if i == items.len() - 1 {
            ""
        } else {
            options.item_seperator
        };

        if addition_len + seperator.len() <= TWO_THOUSAND {
            messages[current_msg].push_str(&format!("{}{}", item, seperator));
        } else {
            messages[current_msg].push_str(options.block_marker);
            messages.push(String::with_capacity(TWO_THOUSAND));
            current_msg += 1;
            messages[current_msg]
                .push_str(&format!("{}{}{}", options.block_marker, &item, seperator));
        }
    }

    if messages[current_msg].len() + options.block_marker.len() + " ".len() + caption.len()
        != TWO_THOUSAND
    {
        messages[current_msg].push_str(options.block_marker);
        messages[current_msg].push_str(&format!(" {}", caption));
    } else {
        messages.push(caption.to_string());
    }

    messages
}

pub fn format_as_numeric_list(items: &[&str]) -> Vec<String> {
    let mut i = 0;
    items
        .iter()
        .map(|s| {
            let numbered = i.to_string()
                + ": "
                + if s.len() > ONE_HUNDRED { "\n" } else { "" }
                + &s.truncate_with_ellipse(ONE_HUNDRED)
                + "\n";
            i += 1;
            numbered
        })
        .collect()
}

const IMAGE_TYPES: [&str; 3] = [".png", ".gif", ".jpg"];
pub fn extract_image_urls(input: &str) -> Vec<&str> {
    let mut urls = Vec::new();
    for word in input.split_whitespace() {
        for image_type in IMAGE_TYPES {
            if word.contains("https://") && word.contains(image_type) {
                urls.push(word);
            }
        }
    }
    urls
}

#[derive(Debug, Copy, Clone)]
pub enum ListStyle {
    CommaSeparatedBlocks,
    Numeric,
    Id,
    None,
}

pub const LIST_STYLE_COMMA_SEPARATED: &str = "comma";
pub const LIST_STYLE_NUMERIC: &str = "numeric";
pub const LIST_STYLE_ID: &str = "id";
pub const LIST_STYLE_NONE: &str = "none";

impl FromStr for ListStyle {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            LIST_STYLE_COMMA_SEPARATED => Ok(ListStyle::CommaSeparatedBlocks),
            LIST_STYLE_NUMERIC => Ok(ListStyle::Numeric),
            LIST_STYLE_ID => Ok(ListStyle::Id),
            LIST_STYLE_NONE => Ok(ListStyle::None),
            _ => Err(format!("Unknown context {}", s)),
        }
    }
}

pub fn format_item_as_id_item<T: Identifiable>(item: &mut T) -> String {
    format!(
        "\nID: {}\n{}{}\n",
        item.id(),
        if item.name().len() > ONE_HUNDRED {
            "\n"
        } else {
            ""
        },
        item.take_name(),
    )
}

pub fn format_item_list<T: Identifiable>(
    items: Vec<T>,
    list_style: ListStyle,
    caption: Option<&str>,
) -> Vec<String> {
    let mut items = items;
    let caption = caption.unwrap_or("");
    let items: Vec<String> = if matches!(list_style, ListStyle::Id) {
        items.iter_mut().map(format_item_as_id_item).collect()
    } else {
        items.iter_mut().map(|item| item.take_name()).collect()
    };

    match list_style {
        ListStyle::CommaSeparatedBlocks => {
            let items = items.as_strs();
            format_as_item_seperated_list(&items, caption, SeperatedListOptions::default())
        }
        ListStyle::Numeric => {
            let items = items.as_strs();
            format_as_numeric_list(&items)
        }
        ListStyle::Id => {
            let items = items.as_strs();
            format_as_item_seperated_list(&items, caption, SeperatedListOptions::none())
        }
        ListStyle::None => {
            let items = items.as_strs();
            format_as_item_seperated_list(&items, caption, SeperatedListOptions::space_seperated())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEM_SEPERATOR: &str = ", ";

    #[test]
    fn split_sub_args() {
        let input = "sub1 sub2 s3 s sub_five";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["sub1", "sub2", "s3", "s", "sub_five"]);
    }

    #[test]
    fn split_sub_args_with_quotes() {
        let input = "sub1 \"sub 2\"       \"sub       three\"";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["sub1", "sub 2", "sub       three"]);
    }

    #[test]
    fn split_sub_args_single() {
        let input = "sub1";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["sub1"]);
    }

    #[test]
    fn split_sub_args_empty_string() {
        let input = "";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, Vec::<&str>::new());
    }

    #[test]
    fn split_sub_args_only_spaces() {
        let input = "     ";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, Vec::<&str>::new());
    }

    #[test]
    fn split_sub_args_leading_spaces() {
        let input = "   sub1 sub2";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["sub1", "sub2"]);
    }

    #[test]
    fn split_sub_args_trailing_spaces() {
        let input = "sub1 sub2   ";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["sub1", "sub2"]);
    }

    #[test]
    fn split_sub_args_quoted_only() {
        let input = "\"hello world\"";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["hello world"]);
    }

    #[test]
    fn split_sub_args_empty_quotes() {
        let input = "sub1 \"\" sub2";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["sub1", "", "sub2"]);
    }

    #[test]
    fn split_sub_args_adjacent_quotes() {
        let input = "\"foo\"\"bar\"";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["foo", "bar"]);
    }

    #[test]
    fn split_sub_args_unclosed_quote() {
        let input = "sub1 \"unclosed";
        let result = parse_bot_args(input);
        assert!(matches!(result, Err(FunboyError::UserInput(_))));
    }

    #[test]
    fn split_sub_args_quote_at_end() {
        let input = "sub1 \"sub2\"";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["sub1", "sub2"]);
    }

    #[test]
    fn split_sub_args_unicode() {
        let input = "héllo wörld";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["héllo", "wörld"]);
    }

    #[test]
    fn split_sub_args_unicode_in_quotes() {
        let input = "\"héllo wörld\" foo";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["héllo wörld", "foo"]);
    }

    #[test]
    fn split_sub_args_multiple_consecutive_spaces() {
        let input = "sub1   sub2   sub3";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["sub1", "sub2", "sub3"]);
    }

    #[test]
    fn split_sub_args_quote_adjacent_to_word() {
        let input = "sub1\"sub 2\"sub3";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["sub1", "sub 2", "sub3"]);
    }

    #[test]
    fn split_sub_args_quote_at_start_no_space() {
        let input = "\"quoted\" unquoted";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["quoted", "unquoted"]);
    }

    #[test]
    fn split_sub_args_tab_separated() {
        let input = "sub1\tsub2\tsub3";
        let result = parse_bot_args(input).unwrap();
        assert_eq!(result, vec!["sub1", "sub2", "sub3"]);
    }

    #[test]
    fn split_sub_args_single_quote() {
        let input = "\"";
        let result = parse_bot_args(input);
        assert!(matches!(result, Err(FunboyError::UserInput(_))));
    }

    #[test]
    fn test_no_words_cut_in_middle() {
        let input = "hello world this isss aaaa test message ".repeat(1000);
        let result = split_message(&input, TWO_THOUSAND);
        for msg in &result {
            dbg!(&msg);
            assert!(!(msg.len() > TWO_THOUSAND));
            assert!(
                msg.ends_with("hello")
                    || msg.ends_with("world")
                    || msg.ends_with("this")
                    || msg.ends_with("isss")
                    || msg.ends_with("aaaa")
                    || msg.ends_with("test")
                    || msg.ends_with("message")
                    || msg.ends_with(" ")
            );
        }
    }

    #[test]
    fn test_messages_reconstruct_original() {
        let input = "The quick brown fox jumps over the lazy dog. ".repeat(100);
        let result = split_message(&input, TWO_THOUSAND);
        let reconstructed = result.join("");
        assert_eq!(reconstructed, input);
    }

    #[test]
    fn test_empty_string() {
        let input = "";
        let result = split_message(input, TWO_THOUSAND);
        assert!(result.is_empty() || (result.len() == 1 && result[0].is_empty()));
    }

    #[test]
    fn test_long_block() {
        let input = "=".repeat(TWO_THOUSAND * 2);
        let result = split_message(&input, TWO_THOUSAND);
        dbg!(&result);
        assert!(result[0].len() == TWO_THOUSAND);
        assert!(result[1].len() == TWO_THOUSAND);
    }

    #[test]
    fn test_single_word() {
        let input = "verylongword";
        let result = split_message(input, TWO_THOUSAND);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "verylongword");
    }

    #[test]
    fn test_multiple_spaces() {
        let input = "hello    world    test";
        let result = split_message(&input, TWO_THOUSAND);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "hello    world    test");
    }

    #[test]
    fn test_newlines_and_tabs() {
        let input = "hello\nworld\ttest";
        let result = split_message(&input, TWO_THOUSAND);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "hello\nworld\ttest");
    }

    #[test]
    fn split_a_long_message() {
        let mut message: Vec<String> = Vec::new();
        let mut long_string = String::with_capacity(23000);

        for _ in 0..23000 {
            long_string.push('0');
        }
        message.push(long_string);

        let mut regular_string = String::with_capacity(1000);
        let mut regular_string_2 = String::with_capacity(2000);
        let mut regular_string_3 = String::with_capacity(1999);
        let mut regular_string_4 = String::with_capacity(2001);

        for _ in 0..1000 {
            regular_string.push('1');
        }
        for _ in 0..2000 {
            regular_string_2.push('2');
        }
        for _ in 0..1999 {
            regular_string_3.push('3');
        }
        for _ in 0..2001 {
            regular_string_4.push('4');
        }

        message.push(regular_string);
        message.push(regular_string_2);
        message.push(regular_string_3);
        message.push(regular_string_4);

        for split in split_messages(
            &message.iter().map(|s| &s[..]).collect::<Vec<&str>>()[..],
            TWO_THOUSAND,
        ) {
            dbg!(split.len());
            assert!(split.len() <= super::TWO_THOUSAND);
        }
    }

    const MARKDOWN: &str = "```";
    const NOTIFY_TEXT: &str = "added to `nothing`";
    const LIMIT: usize = 2000 - NOTIFY_TEXT.len() - (MARKDOWN.len() * 2) - ITEM_SEPERATOR.len();

    #[tokio::test]
    async fn format_sub_logs() {
        let mut test_subs = Vec::new();
        for i in 0..1000 {
            test_subs.push(format!("test {}", i));
        }

        let test_subs: Vec<&str> = test_subs.iter().map(|s| s.as_str()).collect();

        let messages = format_as_item_seperated_list(
            &test_subs,
            "added to `nothing`",
            SeperatedListOptions::default(),
        );

        for message in messages {
            dbg!(&message);
            assert!(message.len() <= TWO_THOUSAND);
        }
    }

    #[tokio::test]
    async fn format_sub_log_seperator() {
        let mut test_subs = Vec::new();
        let mut test_sub = String::new();
        for _ in 0..LIMIT {
            test_sub.push_str("t");
        }

        test_subs.push(test_sub.as_str());

        let mut test_sub = String::new();
        for _ in 0..LIMIT - 10 {
            test_sub.push_str("x");
        }

        test_subs.push(test_sub.as_str());

        let messages =
            format_as_item_seperated_list(&test_subs, NOTIFY_TEXT, SeperatedListOptions::default());

        dbg!(&messages[1]);
        assert!(messages[1].ends_with(&format!("x{} {}", MARKDOWN, NOTIFY_TEXT)));

        for message in messages {
            assert!(message.len() <= TWO_THOUSAND);
        }
    }

    #[tokio::test]
    async fn format_sub_log_seperator_second_block() {
        const TEST_CASE: &str = "from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx";
        let test_case: Vec<&str> = TEST_CASE.split_whitespace().collect();
        let messages =
            format_as_item_seperated_list(&test_case, NOTIFY_TEXT, SeperatedListOptions::default());
        dbg!(&messages[1]);
        assert!(messages[1].starts_with("```xxxxxxx, from:"));
    }

    #[tokio::test]
    async fn format_long_sub_log() {
        let mut test_sub = String::with_capacity(2001);
        for _ in 0..2001 {
            test_sub.push_str("s ");
        }

        let messages = format_as_item_seperated_list(
            &[&test_sub],
            NOTIFY_TEXT,
            SeperatedListOptions::default(),
        );

        for message in messages {
            dbg!(&message);
            assert!(message.len() <= TWO_THOUSAND);
        }

        let mut test_sub = String::with_capacity(LIMIT);
        for _ in 0..LIMIT {
            test_sub.push_str("s");
        }

        let messages = format_as_item_seperated_list(
            &[&test_sub],
            NOTIFY_TEXT,
            SeperatedListOptions::default(),
        );

        for message in messages {
            dbg!(&message);
            assert!(message.len() <= TWO_THOUSAND);
        }

        let edge_case = LIMIT - 1;
        let mut test_sub = String::with_capacity(edge_case);
        for _ in 0..edge_case {
            test_sub.push_str("s");
        }

        let messages = format_as_item_seperated_list(
            &[&test_sub],
            NOTIFY_TEXT,
            SeperatedListOptions::default(),
        );

        for message in messages {
            dbg!(&message);
            assert!(message.len() <= TWO_THOUSAND);
        }
    }
}
