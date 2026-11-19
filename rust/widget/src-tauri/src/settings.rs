//! `%LOCALAPPDATA%\AIUsageTracker\widget.json`: the same file and keys as the Python widget
//! (`x`, `y`, `topmost`, `alerts`), so its settings carry over. Keys this version does not
//! know are kept as they are.

use crate::view::truthy;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Settings {
    path: PathBuf,
    data: Map<String, Value>,
}

impl Settings {
    /// `load_settings`: a missing or unreadable file is simply no settings.
    pub fn load(path: &Path) -> Settings {
        let text = std::fs::read_to_string(path).ok();
        let data = match text.and_then(|text| serde_json::from_str(&text).ok()) {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        Settings {
            path: path.to_path_buf(),
            data,
        }
    }

    /// `save_settings`: a file that cannot be written is not an error.
    pub fn save(&self) {
        let folder = self.path.parent().map_or(Ok(()), std::fs::create_dir_all);
        let text = Value::Object(self.data.clone()).to_string();
        let _ = folder.and_then(|()| std::fs::write(&self.path, text));
    }

    /// `settings.get(key, default)` used as an on/off switch.
    pub fn flag(&self, key: &str, default: bool) -> bool {
        self.data.get(key).map_or(default, truthy)
    }

    pub fn set_flag(&mut self, key: &str, on: bool) {
        self.data.insert(key.into(), Value::Bool(on));
    }

    /// The saved window position; either half may be missing.
    pub fn position(&self) -> (Option<i64>, Option<i64>) {
        let whole = |key: &str| {
            let v = self.data.get(key)?;
            v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64))
        };
        (whole("x"), whole("y"))
    }

    pub fn set_position(&mut self, x: i64, y: i64) {
        self.data.insert("x".into(), x.into());
        self.data.insert("y".into(), y.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("aiut-widget-{}-{name}", std::process::id()))
            .join("AIUsageTracker")
            .join("widget.json")
    }

    #[test]
    fn missing_or_broken_file_gives_the_defaults() {
        let path = temp("missing");
        let settings = Settings::load(&path);
        assert_eq!(settings.position(), (None, None));
        assert!(settings.flag("topmost", true));
        assert!(settings.flag("alerts", true));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for broken in ["{not json", "[1, 2]", ""] {
            std::fs::write(&path, broken).unwrap();
            assert_eq!(Settings::load(&path).position(), (None, None));
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn round_trip_creates_the_folder_and_keeps_unknown_keys() {
        let path = temp("round-trip");
        let mut settings = Settings::load(&path);
        settings.set_position(-1500, 200);
        settings.set_flag("topmost", false);
        settings.save();
        let again = Settings::load(&path);
        assert_eq!(again.position(), (Some(-1500), Some(200)));
        assert!(!again.flag("topmost", true));
        assert!(again.flag("alerts", true));

        // what the Python widget wrote, plus a key from some other version
        std::fs::write(
            &path,
            r#"{"x": 100, "y": 80, "topmost": true, "alerts": false, "later": [1]}"#,
        )
        .unwrap();
        let mut python = Settings::load(&path);
        assert_eq!(python.position(), (Some(100), Some(80)));
        assert!(python.flag("topmost", false));
        assert!(!python.flag("alerts", true));
        python.set_flag("alerts", true);
        python.save();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved,
            serde_json::json!({"x": 100, "y": 80, "topmost": true, "alerts": true, "later": [1]})
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn an_unwritable_place_is_not_an_error() {
        let file = temp("unwritable");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{}").unwrap();
        // a path *through* a file can never be created
        let mut settings = Settings::load(&file.join("widget.json"));
        settings.set_position(1, 2);
        settings.save();
        assert_eq!(settings.position(), (Some(1), Some(2)));
        let _ = std::fs::remove_dir_all(file.parent().unwrap().parent().unwrap());
    }
}
