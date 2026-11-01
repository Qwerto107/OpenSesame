use std::fmt;

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

pub const SYSTEM: &str = "system";

pub fn resolve(preference: &str) -> &'static str {
    let locale = if preference == SYSTEM {
        sys_locale::get_locale().unwrap_or_else(|| "en".into())
    } else {
        preference.to_owned()
    }
    .replace('_', "-");
    LANGUAGES
        .iter()
        .find(|(tag, _)| tag.eq_ignore_ascii_case(&locale))
        .or_else(|| {
            LANGUAGES.iter().find(|(tag, _)| {
                tag.split('-')
                    .next()
                    .unwrap()
                    .eq_ignore_ascii_case(locale.split('-').next().unwrap())
            })
        })
        .map(|(tag, _)| *tag)
        .unwrap_or("en")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    source: String,
    arguments: Vec<(&'static str, Argument)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Argument {
    Text(String),
    Message(Box<Message>),
}

impl Message {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            arguments: vec![],
        }
    }

    pub fn with(mut self, name: &'static str, value: impl Into<Argument>) -> Self {
        self.arguments.push((name, value.into()));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.source.is_empty()
    }
    pub fn clear(&mut self) {
        self.source.clear();
        self.arguments.clear();
    }

    pub fn render(&self, language: &str) -> String {
        let mut template = lookup(language, &self.source);
        let mut output = String::new();
        // 只展开模板中的参数，用户名称或系统错误中的花括号保持原样。
        while let Some(start) = template.find('{') {
            let Some(end) = template[start..].find('}').map(|end| start + end) else {
                break;
            };
            output.push_str(&template[..start]);
            let name = &template[start + 1..end];
            if let Some((_, value)) = self.arguments.iter().find(|(key, _)| *key == name) {
                match value {
                    Argument::Text(text) => output.push_str(text),
                    Argument::Message(message) => output.push_str(&message.render(language)),
                }
            } else {
                output.push_str(&template[start..=end]);
            }
            template = &template[end + 1..];
        }
        output.push_str(template);
        output
    }
}

impl From<&str> for Message {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}
impl From<String> for Message {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}
impl From<String> for Argument {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl From<&str> for Argument {
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}
impl From<Message> for Argument {
    fn from(value: Message) -> Self {
        Self::Message(Box::new(value))
    }
}
macro_rules! numeric_argument {
    ($($ty:ty),*) => { $(impl From<$ty> for Argument { fn from(value: $ty) -> Self { Self::Text(value.to_string()) } })* };
}
numeric_argument!(usize, u64, u32, u16, i32);

impl fmt::Display for Message {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render("en"))
    }
}
impl std::error::Error for Message {}

#[macro_export]
macro_rules! msg {
    ($source:expr $(, $key:literal => $value:expr)* $(,)?) => {
        $crate::i18n::Message::new($source)$(.with($key, $value))*
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translation_fallback_and_literal_arguments() {
        let message = crate::msg!("Server: {name}", "name" => "{count} 中文名称");
        assert_eq!(message.render("en"), "Server: {count} 中文名称");
        assert_eq!(
            Message::from("Missing translation").render("unknown"),
            "Missing translation"
        );
        assert_eq!(Message::from("Save").render("zh-CN"), "保存");
        assert_eq!(crate::msg!("{field} must be a positive integer.", "field" => Message::from("Step interval")).render("zh-CN"), "步骤间隔须为有效的正整数");
        assert_eq!(
            Message::from("unmatched { input").render("en"),
            "unmatched { input"
        );
        assert_eq!(resolve("en-US"), "en");
        assert_eq!(resolve("zh_TW"), "zh-CN");
        assert_eq!(resolve("unknown"), "en");
    }
}
