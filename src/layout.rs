//! Layout edits for the control API: small, named changes to a [`Config`],
//! each validated whole against the model before it may be written.
//!
//! Pure: no file, no device. The control API reads the config, applies one
//! [`Edit`], and writes the result (see `docs/control-api.md`).

use crate::config::{resolve_page_target, ButtonConfig, Config, Page};
use crate::model::Model;

/// One change a caller asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Replace key `key` on `page` whole with `button` (its `key` is set here).
    SetKey {
        page: String,
        key: u8,
        button: Box<ButtonConfig>,
    },
    /// Blank key `key` on `page`; already blank is fine.
    ClearKey { page: String, key: u8 },
    /// Append a page called `name`.
    AddPage { name: String },
    /// Remove `page`.
    RemovePage { page: String },
    /// Store `0..=100` as the layout's brightness.
    Brightness(u8),
}

/// Why an edit was refused; each maps to a named refusal of the API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// `page` names no page.
    NoSuchPage(String),
    /// Removing the only page.
    LastPage,
    /// The layout the edit would make breaks a rule.
    Invalid(String),
}

impl EditError {
    /// The refusal's name, as the API says it.
    pub fn name(&self) -> &'static str {
        match self {
            EditError::NoSuchPage(_) => "no-such-page",
            EditError::LastPage => "last-page",
            EditError::Invalid(_) => "invalid",
        }
    }

    /// What went wrong, in words.
    pub fn detail(&self) -> String {
        match self {
            EditError::NoSuchPage(page) => format!("no page '{page}'"),
            EditError::LastPage => "a layout keeps at least one page".into(),
            EditError::Invalid(why) => why.clone(),
        }
    }
}

/// What an applied edit produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub config: Config,
    /// The index of the page an `AddPage` appended.
    pub page: Option<usize>,
}

/// Apply `edit` to `config`, and validate the result against `model`.
pub fn apply(config: &Config, edit: Edit, model: &Model) -> Result<Applied, EditError> {
    let mut pages = config.pages();
    let mut added = None;
    let mut brightness = config.brightness;

    match edit {
        Edit::SetKey { page, key, button } => {
            let index = find_page(&pages, &page)?;
            let buttons = &mut pages[index].buttons;
            buttons.retain(|b| b.key != key);
            buttons.push(ButtonConfig { key, ..*button });
            buttons.sort_by_key(|b| b.key);
        }
        Edit::ClearKey { page, key } => {
            let index = find_page(&pages, &page)?;
            pages[index].buttons.retain(|b| b.key != key);
        }
        Edit::AddPage { name } => {
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(EditError::Invalid("a page needs a name".into()));
            }
            if pages
                .iter()
                .any(|p| p.name.as_deref() == Some(name.as_str()))
            {
                return Err(EditError::Invalid(format!(
                    "a page is already called '{name}'"
                )));
            }
            if name.parse::<usize>().is_ok() {
                return Err(EditError::Invalid(format!(
                    "'{name}' reads as a page index; name a page with words"
                )));
            }
            pages.push(Page {
                name: Some(name),
                buttons: Vec::new(),
            });
            added = Some(pages.len() - 1);
        }
        Edit::RemovePage { page } => {
            let index = find_page(&pages, &page)?;
            if pages.len() == 1 {
                return Err(EditError::LastPage);
            }
            pages.remove(index);
        }
        Edit::Brightness(value) => brightness = Some(value),
    }

    let next = rebuild(config, pages, brightness);
    next.validate(model)
        .map_err(|err| EditError::Invalid(invalid_detail(err)))?;
    Ok(Applied {
        config: next,
        page: added,
    })
}

/// A layout's revision: the FNV-1a 64-bit hash of the config file's bytes.
pub fn revision(bytes: &[u8]) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let hash = bytes
        .iter()
        .fold(OFFSET, |h, b| (h ^ u64::from(*b)).wrapping_mul(PRIME));
    format!("{hash:016x}")
}

fn find_page(pages: &[Page], target: &str) -> Result<usize, EditError> {
    let names: Vec<Option<String>> = pages.iter().map(|p| p.name.clone()).collect();
    resolve_page_target(target, pages.len(), &names)
        .ok_or_else(|| EditError::NoSuchPage(target.to_string()))
}

/// Keep the single-page shorthand while there is one unnamed page.
fn rebuild(config: &Config, pages: Vec<Page>, brightness: Option<u8>) -> Config {
    let single = pages.len() == 1 && pages[0].name.is_none();
    let (buttons, pages) = if single {
        (
            pages.into_iter().next().unwrap_or_default().buttons,
            Vec::new(),
        )
    } else {
        (Vec::new(), pages)
    };
    Config {
        brightness,
        press_feedback: config.press_feedback,
        buttons,
        pages,
    }
}

fn invalid_detail(err: crate::Error) -> String {
    match err {
        crate::Error::ConfigInvalid(why) => why,
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labelled(key: u8, label: &str) -> ButtonConfig {
        ButtonConfig {
            key,
            label: Some(label.into()),
            ..Default::default()
        }
    }

    fn single(buttons: Vec<ButtonConfig>) -> Config {
        Config {
            brightness: Some(40),
            press_feedback: None,
            buttons,
            pages: Vec::new(),
        }
    }

    fn set(page: &str, key: u8, button: ButtonConfig) -> Edit {
        Edit::SetKey {
            page: page.into(),
            key,
            button: Box::new(button),
        }
    }

    #[test]
    fn sets_a_key_whole_replacing_the_old_and_keeps_the_shorthand() {
        let config = single(vec![
            ButtonConfig {
                run: Some("old".into()),
                ..labelled(3, "Old")
            },
            labelled(1, "One"),
        ]);
        let applied = apply(&config, set("0", 3, labelled(0, "New")), &Model::MK2).unwrap();
        assert!(applied.config.pages.is_empty());
        assert_eq!(
            applied.config.buttons,
            vec![labelled(1, "One"), labelled(3, "New")]
        );
        assert_eq!(applied.config.brightness, Some(40));
    }

    #[test]
    fn clears_a_key_and_clearing_a_blank_key_is_fine() {
        let config = single(vec![labelled(2, "Two")]);
        let once = apply(
            &config,
            Edit::ClearKey {
                page: "0".into(),
                key: 2,
            },
            &Model::MK2,
        )
        .unwrap();
        assert!(once.config.buttons.is_empty());
        let twice = apply(
            &once.config,
            Edit::ClearKey {
                page: "0".into(),
                key: 2,
            },
            &Model::MK2,
        );
        assert_eq!(twice.unwrap().config, once.config);
    }

    #[test]
    fn adds_a_page_turning_the_shorthand_into_pages() {
        let config = single(vec![labelled(0, "Home")]);
        let applied = apply(
            &config,
            Edit::AddPage {
                name: " Media ".into(),
            },
            &Model::MK2,
        )
        .unwrap();
        assert_eq!(applied.page, Some(1));
        assert!(applied.config.buttons.is_empty());
        assert_eq!(applied.config.pages.len(), 2);
        assert_eq!(applied.config.pages[0].name, None);
        assert_eq!(applied.config.pages[0].buttons, vec![labelled(0, "Home")]);
        assert_eq!(applied.config.pages[1].name.as_deref(), Some("Media"));
    }

    #[test]
    fn sets_a_key_on_a_page_named_or_indexed() {
        let config = apply(
            &single(vec![]),
            Edit::AddPage {
                name: "Media".into(),
            },
            &Model::MK2,
        )
        .unwrap()
        .config;
        let by_name = apply(&config, set("Media", 4, labelled(0, "Play")), &Model::MK2).unwrap();
        let by_index = apply(&config, set("1", 4, labelled(0, "Play")), &Model::MK2).unwrap();
        assert_eq!(by_name.config, by_index.config);
        assert_eq!(by_name.config.pages[1].buttons, vec![labelled(4, "Play")]);
    }

    #[test]
    fn removes_a_page_and_folds_back_to_the_shorthand() {
        let config = apply(
            &single(vec![labelled(0, "Home")]),
            Edit::AddPage {
                name: "Media".into(),
            },
            &Model::MK2,
        )
        .unwrap()
        .config;
        let applied = apply(
            &config,
            Edit::RemovePage {
                page: "Media".into(),
            },
            &Model::MK2,
        )
        .unwrap();
        assert!(applied.config.pages.is_empty());
        assert_eq!(applied.config.buttons, vec![labelled(0, "Home")]);
    }

    #[test]
    fn stores_brightness() {
        let applied = apply(&single(vec![]), Edit::Brightness(80), &Model::MK2).unwrap();
        assert_eq!(applied.config.brightness, Some(80));
    }

    #[test]
    fn refuses_each_edit_by_name() {
        let config = single(vec![labelled(0, "Home")]);
        let cases = [
            (set("Nope", 0, labelled(0, "x")), "no-such-page"),
            (set("0", 99, labelled(0, "x")), "invalid"),
            (set("0", 1, ButtonConfig::default()), "invalid"),
            (
                set(
                    "0",
                    1,
                    ButtonConfig {
                        builtin: Some("page:Nope".into()),
                        ..labelled(1, "x")
                    },
                ),
                "invalid",
            ),
            (
                Edit::ClearKey {
                    page: "3".into(),
                    key: 0,
                },
                "no-such-page",
            ),
            (Edit::AddPage { name: "  ".into() }, "invalid"),
            (Edit::AddPage { name: "2".into() }, "invalid"),
            (Edit::RemovePage { page: "0".into() }, "last-page"),
            (Edit::RemovePage { page: "x".into() }, "no-such-page"),
            (Edit::Brightness(101), "invalid"),
        ];
        for (edit, name) in cases {
            let err = apply(&config, edit.clone(), &Model::MK2).unwrap_err();
            assert_eq!(err.name(), name, "{edit:?}: {}", err.detail());
            assert!(!err.detail().is_empty());
        }
    }

    #[test]
    fn refuses_a_second_page_of_the_same_name() {
        let config = apply(
            &single(vec![]),
            Edit::AddPage {
                name: "Media".into(),
            },
            &Model::MK2,
        )
        .unwrap()
        .config;
        let err = apply(
            &config,
            Edit::AddPage {
                name: "Media".into(),
            },
            &Model::MK2,
        )
        .unwrap_err();
        assert_eq!(err.name(), "invalid");
    }

    #[test]
    fn hashes_bytes_into_a_stable_sixteen_digit_revision() {
        assert_eq!(revision(b""), "cbf29ce484222325");
        assert_eq!(revision(b"a"), "af63dc4c8601ec8c");
        assert_ne!(
            revision(b"brightness = 40\n"),
            revision(b"brightness = 41\n")
        );
    }
}
