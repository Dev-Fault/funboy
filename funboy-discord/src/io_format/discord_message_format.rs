use super::quote_filter::QuoteFilter;

pub const DISCORD_CHARACTER_LIMIT: usize = 2000;

/// Split input by whitespace unless surrounded by quotes
pub fn split_by_whitespace_unless_quoted(input: &str) -> Vec<&str> {
    let quote_filter = &QuoteFilter::from(input);

    let mut output: Vec<&str> = Vec::new();

    for quoted in &quote_filter.quoted {
        output.push(quoted);
    }

    for unquoted in &quote_filter.unquoted {
        for word in unquoted.split_whitespace() {
            output.push(word);
        }
    }

    output
}

pub fn split_message(message: &[&str]) -> Vec<String> {
    let mut message_split: Vec<String> = Vec::new();

    let iter = message.iter();
    let mut message_part: String = String::default();

    for value in iter {
        if message_part.len() + value.len() <= DISCORD_CHARACTER_LIMIT {
            message_part.push_str(value);
        } else {
            message_split.push(message_part);
            message_part = String::default();
            if value.len() <= DISCORD_CHARACTER_LIMIT {
                message_part.push_str(value);
            } else {
                for sub_str in split_long_string(value) {
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

pub fn split_block<'a>(str: &'a str) -> Vec<&'a str> {
    let mut output = Vec::new();
    let blocks: usize = str.len() / DISCORD_CHARACTER_LIMIT;

    for i in 0..blocks {
        output.push(&str[i * DISCORD_CHARACTER_LIMIT..(i + 1) * DISCORD_CHARACTER_LIMIT]);
    }

    if blocks * DISCORD_CHARACTER_LIMIT < str.len() {
        output.push(&str[blocks * DISCORD_CHARACTER_LIMIT..str.len()]);
    }

    output
}

// TODO: Fix bugs with this
pub fn split_long_string(str: &str) -> Vec<&str> {
    let mut output = Vec::new();

    let mut output_length = 0;
    let mut message_length = 0;

    for word in str.split_inclusive(' ').collect::<Vec<&str>>() {
        if message_length + word.len() > DISCORD_CHARACTER_LIMIT {
            output.push(
                str.get(output_length..output_length + message_length)
                    .unwrap_or_default(),
            );
            output_length += message_length;
            message_length = 0;
        }
        message_length += word.len();
    }

    if let Some(o) = str.get(output_length..output_length + message_length) {
        output.push(o)
    }

    let mut output2 = Vec::new();
    for message in output {
        if message.len() > DISCORD_CHARACTER_LIMIT {
            for block in split_block(message) {
                output2.push(block);
            }
        } else {
            output2.push(message);
        }
    }

    output2
}

pub fn ellipsize_if_long(item: &str, limit: usize) -> String {
    if limit > item.len() {
        item.to_string()
    } else {
        match item.get(0..limit) {
            Some(substr) => substr.to_owned() + "...",
            None => String::new(),
        }
    }
}

// TODO: generalize this so that MARKDOWN and ITEM_SEPERATOR can be passed to function
pub const MARKDOWN: &str = "```";
pub const ITEM_SEPERATOR: &str = ", ";
pub fn format_as_item_seperated_list(items: &[&str], appended_text: &str) -> Vec<String> {
    let mut messages: Vec<String> = Vec::new();
    messages.push(String::with_capacity(DISCORD_CHARACTER_LIMIT));
    let mut current_msg = 0;

    messages[current_msg].push_str(MARKDOWN);
    for (i, item) in items.iter().enumerate() {
        let item = item.to_string();

        let item = if item.contains(char::is_whitespace) {
            format!("\"{}\"", item)
        } else {
            format!("{}", item)
        };

        let item = if item.len()
            > DISCORD_CHARACTER_LIMIT
                - (MARKDOWN.len() * 2)
                - appended_text.len()
                - ITEM_SEPERATOR.len()
        {
            format!("\"{}\"", ellipsize_if_long(&item, 255))
        } else {
            item
        };

        let addition_len = messages[current_msg].len() + item.len() + MARKDOWN.len();

        let seperator = if i == items.len() - 1 {
            ""
        } else {
            ITEM_SEPERATOR
        };

        if addition_len + seperator.len() <= DISCORD_CHARACTER_LIMIT {
            messages[current_msg].push_str(&format!("{}{}", item, seperator));
        } else {
            messages[current_msg].push_str(MARKDOWN);
            messages.push(String::with_capacity(DISCORD_CHARACTER_LIMIT));
            current_msg += 1;
            messages[current_msg].push_str(&format!("{}{}{}", MARKDOWN, &item, seperator));
        }
    }

    if messages[current_msg].len() + MARKDOWN.len() + " ".len() + appended_text.len()
        != DISCORD_CHARACTER_LIMIT
    {
        messages[current_msg].push_str(MARKDOWN);
        messages[current_msg].push_str(&format!(" {}", appended_text));
    } else {
        messages.push(appended_text.to_string());
    }

    messages
}

pub fn format_as_standard_list(items: &[&str]) -> Vec<String> {
    items
        .iter()
        .map(|s| {
            if s.len() > DISCORD_CHARACTER_LIMIT / 10 {
                "\n".to_string() + s + "\n"
            } else if s.contains(' ') {
                format!("{}{}{}", "[", s, "] ")
            } else {
                s.to_string() + " "
            }
        })
        .collect()
}

pub fn format_as_numeric_list(items: &[&str]) -> Vec<String> {
    let mut i = 0;
    items
        .iter()
        .map(|s| {
            let numbered = i.to_string() + ": " + s + "\n";
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_quote_input() {
        let input = String::from(
            "cat \"\" \"United States of America\" bear snake lion \"my mom\"  \"ten bulls\" dog goat",
        );

        // dbg!(&vectorize_input(&input));

        assert_eq!(split_by_whitespace_unless_quoted(&input).len(), 9);
    }

    #[test]
    fn no_quote_input() {
        let input = String::from("This is some input");

        assert_eq!(split_by_whitespace_unless_quoted(&input).len(), 4);
    }

    #[test]
    fn split_a_long_string() {
        let mut long_string = String::with_capacity(23000);

        for _ in 0..23000 {
            long_string.push('0');
        }

        let split_string = split_long_string(&long_string);

        let mut character_count = 0;
        for s in &split_string {
            character_count += s.len();
        }
        assert!(character_count == long_string.len());

        for s in &split_string {
            assert!(s.len() <= super::DISCORD_CHARACTER_LIMIT);
        }
    }

    #[test]
    fn split_a_long_string_with_spaces() {
        let mut long_string = String::with_capacity(23000);

        for i in 0..23000 {
            // also test with spaces at random positions
            if i == 2004 || i == 4500 {
                long_string.push(' ');
            } else {
                long_string.push('0');
            }
        }

        long_string.insert_str(
            8438,
            " some normal words that you would find in any message on discord ",
        );

        dbg!(&long_string);

        let split_string = split_long_string(&long_string);

        let mut character_count = 0;
        for s in &split_string {
            println!("\nMessage: {}\nLength: {}\n\n", &s, &s.len());
            character_count += s.len();
            assert!(s.len() <= super::DISCORD_CHARACTER_LIMIT);
        }
        println!(
            "Count: {} actual length: {}",
            &character_count,
            &long_string.len()
        );
        assert!(character_count == long_string.len());
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

        for split in split_message(&message.iter().map(|s| &s[..]).collect::<Vec<&str>>()[..]) {
            dbg!(split.len());
            assert!(split.len() <= super::DISCORD_CHARACTER_LIMIT);
        }
    }

    const NOTIFY_TEXT: &str = "added to `nothing`";
    const LIMIT: usize = 2000 - NOTIFY_TEXT.len() - (MARKDOWN.len() * 2) - ITEM_SEPERATOR.len();

    #[tokio::test]
    async fn format_sub_logs() {
        let mut test_subs = Vec::new();
        for i in 0..1000 {
            test_subs.push(format!("test {}", i));
        }

        let test_subs: Vec<&str> = test_subs.iter().map(|s| s.as_str()).collect();

        let messages = format_as_item_seperated_list(&test_subs, "added to `nothing`");

        for message in messages {
            dbg!(&message);
            assert!(message.len() <= DISCORD_CHARACTER_LIMIT);
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

        let messages = format_as_item_seperated_list(&test_subs, NOTIFY_TEXT);

        dbg!(&messages[1]);
        assert!(messages[1].ends_with(&format!("x{} {}", MARKDOWN, NOTIFY_TEXT)));

        for message in messages {
            assert!(message.len() <= DISCORD_CHARACTER_LIMIT);
        }
    }

    #[tokio::test]
    async fn format_sub_log_seperator_second_block() {
        const TEST_CASE: &str = "from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx from: xxxxxxx";
        let test_case: Vec<&str> = TEST_CASE.split_whitespace().collect();
        let messages = format_as_item_seperated_list(&test_case, NOTIFY_TEXT);
        dbg!(&messages[1]);
        assert!(messages[1].starts_with("```xxxxxxx, from:"));
    }

    #[tokio::test]
    async fn format_long_sub_log() {
        let mut test_sub = String::with_capacity(2001);
        for _ in 0..2001 {
            test_sub.push_str("s ");
        }

        let messages = format_as_item_seperated_list(&[&test_sub], NOTIFY_TEXT);

        for message in messages {
            dbg!(&message);
            assert!(message.len() <= DISCORD_CHARACTER_LIMIT);
        }

        let mut test_sub = String::with_capacity(LIMIT);
        for _ in 0..LIMIT {
            test_sub.push_str("s");
        }

        let messages = format_as_item_seperated_list(&[&test_sub], NOTIFY_TEXT);

        for message in messages {
            dbg!(&message);
            assert!(message.len() <= DISCORD_CHARACTER_LIMIT);
        }

        let edge_case = LIMIT - 1;
        let mut test_sub = String::with_capacity(edge_case);
        for _ in 0..edge_case {
            test_sub.push_str("s");
        }

        let messages = format_as_item_seperated_list(&[&test_sub], NOTIFY_TEXT);

        for message in messages {
            dbg!(&message);
            assert!(message.len() <= DISCORD_CHARACTER_LIMIT);
        }
    }
}
