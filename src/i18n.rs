//! Explicit application messages; user paths, addresses and service diagnostics stay intact.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::LazyLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "it")]
    Italian,
    #[default]
    #[serde(rename = "en", other)]
    English,
}
impl Language {
    pub const ALL: [Self; 6] = [
        Self::English,
        Self::Russian,
        Self::French,
        Self::German,
        Self::Spanish,
        Self::Italian,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Spanish => "Español",
            Self::Italian => "Italiano",
        }
    }
    fn index(self) -> Option<usize> {
        match self {
            Self::English => None,
            Self::Russian => Some(0),
            Self::French => Some(1),
            Self::German => Some(2),
            Self::Spanish => Some(3),
            Self::Italian => Some(4),
        }
    }
}
static CATALOG: LazyLock<BTreeMap<String, [String; 5]>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("locales.json"))
        .expect("Embedded translation catalog must be valid")
});

pub fn tr(language: Language, english: &str) -> &str {
    language
        .index()
        .and_then(|index| CATALOG.get(english).map(|values| values[index].as_str()))
        .unwrap_or(english)
}

/// Arguments are inserted once, never interpreted as templates or translated as application text.
pub fn text(language: Language, english: &str, arguments: &[String]) -> String {
    let mut rest = tr(language, english);
    let mut result = String::new();
    let mut arguments = arguments.iter();
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else {
            result.push_str(&rest[start..]);
            return result;
        };
        match arguments.next() {
            Some(value) => result.push_str(value),
            None => result.push_str(&rest[start..start + end + 1]),
        }
        rest = &rest[start + end + 1..];
    }
    result.push_str(rest);
    result
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    template: String,
    arguments: Vec<String>,
}
impl Message {
    pub fn new(template: impl Into<String>, arguments: Vec<String>) -> Self {
        Self {
            template: template.into(),
            arguments,
        }
    }
    pub fn render(&self, language: Language) -> String {
        text(language, &self.template, &self.arguments)
    }
    pub fn is_empty(&self) -> bool {
        self.template.is_empty()
    }
    pub fn clear(&mut self) {
        self.template.clear();
        self.arguments.clear();
    }
}
impl From<String> for Message {
    fn from(value: String) -> Self {
        Self::new(value, vec![])
    }
}
impl From<&str> for Message {
    fn from(value: &str) -> Self {
        Self::new(value, vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn placeholders(value: &str) -> Vec<&str> {
        value
            .split('{')
            .skip(1)
            .map(|part| part.split('}').next().unwrap())
            .collect()
    }
    #[test]
    fn every_catalog_message_has_all_languages_and_matching_arguments() {
        assert!(CATALOG.len() > 150);
        for (english, translations) in CATALOG.iter() {
            for translation in translations {
                assert!(
                    !translation.trim().is_empty(),
                    "Missing translation: {english}"
                );
                assert_eq!(
                    placeholders(english),
                    placeholders(translation),
                    "Argument mismatch: {english}"
                );
            }
        }
    }
    #[test]
    fn switching_languages_keeps_dynamic_data_literal() {
        let message = Message::new("Settings and usage: {}", vec!["C:/данные/{VERSION}".into()]);
        assert_eq!(
            message.render(Language::French),
            "Paramètres et utilisation : C:/данные/{VERSION}"
        );
        assert_eq!(
            message.render(Language::English),
            "Settings and usage: C:/данные/{VERSION}"
        );
        assert_eq!(
            tr(Language::Russian, "Unrecognized service detail"),
            "Unrecognized service detail"
        );
    }
}
