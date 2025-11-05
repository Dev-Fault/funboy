#[allow(dead_code)]
pub trait StrExtension {
    fn split_inside<'a>(&'a self, begin: char, end: char) -> Option<&'a str>;
    fn starts_with_any(&self, slice: Vec<&str>) -> bool;
}

impl StrExtension for &str {
    fn split_inside<'a>(&'a self, first: char, last: char) -> Option<&'a str> {
        if let (Some(start), Some(end)) = (self.find(first), self.rfind(last)) {
            if start + 1 < end {
                return Some(&self[start + 1..end]);
            }
        }
        None
    }

    fn starts_with_any(&self, strings: Vec<&str>) -> bool {
        for s in strings {
            if s.starts_with(s) {
                return true;
            }
        }
        false
    }
}
