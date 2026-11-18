//! settings.json store (core-app 2.12): one flat JSON object, rewritten
//! whole through `settings.json.new` and a rename. Unknown keys survive.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::io;
use std::path::{Path, PathBuf};

/// The prefs file. `path == None` keeps everything in memory (tests, `--seconds` smoke runs).
#[derive(Clone, Debug, Default)]
pub struct Prefs {
    path: Option<PathBuf>,
    map: Map<String, Value>,
}

impl Prefs {
    /// In-memory store that never touches disk.
    pub fn memory(map: Map<String, Value>) -> Self {
        Self { path: None, map }
    }

    /// Loads `path`; any read or parse error gives an empty store.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let map = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .unwrap_or_default();
        Self { path: Some(path), map }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn map(&self) -> &Map<String, Value> {
        &self.map
    }

    pub fn get_str(&self, k: &str) -> Option<&str> {
        self.map.get(k).and_then(Value::as_str)
    }

    /// True only for JSON `true`.
    pub fn get_bool(&self, k: &str) -> bool {
        self.map.get(k) == Some(&Value::Bool(true))
    }

    pub fn get_f64(&self, k: &str) -> Option<f64> {
        self.map.get(k).and_then(Value::as_f64)
    }

    /// Typed read; absent or malformed -> `None`.
    pub fn get<T: DeserializeOwned>(&self, k: &str) -> Option<T> {
        self.map.get(k).and_then(|v| T::deserialize(v).ok())
    }

    /// Sets one key (`None` removes it) and saves.
    pub fn set(&mut self, k: &str, v: Option<Value>) -> io::Result<()> {
        match v {
            Some(v) => self.map.insert(k.to_owned(), v),
            None => self.map.remove(k),
        };
        self.save()
    }

    /// Serialises `v` under `k` and saves.
    pub fn put<T: Serialize>(&mut self, k: &str, v: &T) -> io::Result<()> {
        let v = serde_json::to_value(v).map_err(io::Error::other)?;
        self.set(k, Some(v))
    }

    /// Edits several keys and saves once.
    pub fn edit(&mut self, f: impl FnOnce(&mut Map<String, Value>)) -> io::Result<()> {
        f(&mut self.map);
        self.save()
    }

    /// Atomic write: folder, `<file>.new`, rename over the file.
    pub fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        path.parent().map(std::fs::create_dir_all).transpose()?;
        let mut tmp = path.clone().into_os_string();
        tmp.push(".new");
        let tmp = PathBuf::from(tmp);
        let bytes = serde_json::to_vec(&self.map).map_err(io::Error::other)?;
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fp-core-prefs-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn round_trip_on_disk() {
        let dir = temp("rt");
        let file = dir.join("sub").join("settings.json");
        let mut p = Prefs::load(&file);
        assert!(p.map().is_empty());
        assert_eq!(p.path(), Some(file.as_path()));
        p.set("a", Some(json!("x"))).unwrap();
        p.put("b", &vec![1, 2]).unwrap();
        p.edit(|m| {
            m.insert("c".into(), json!(true));
        })
        .unwrap();
        p.set("a", None).unwrap();
        let q = Prefs::load(&file);
        assert_eq!(q.get::<Vec<i32>>("b"), Some(vec![1, 2]));
        assert!(q.get_bool("c"));
        assert_eq!(q.get_str("a"), None);
        assert!(!file.with_extension("json.new").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_files_and_typed_reads() {
        let dir = temp("bad");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("settings.json");
        std::fs::write(&f, b"[1,2]").unwrap();
        assert!(Prefs::load(&f).map().is_empty());
        std::fs::write(&f, b"{oops").unwrap();
        assert!(Prefs::load(&f).map().is_empty());
        let p = Prefs::memory(json!({"n": 2.5, "s": "t", "b": 1}).as_object().unwrap().clone());
        assert_eq!(p.get_f64("n"), Some(2.5));
        assert_eq!(p.get_str("s"), Some("t"));
        assert!(!p.get_bool("b"));
        assert_eq!(p.get::<String>("n"), None);
        assert!(p.path().is_none());
        p.save().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_errors_propagate() {
        let dir = temp("err");
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("file");
        std::fs::write(&blocker, b"x").unwrap();
        // parent is a file -> create_dir_all fails
        let mut p = Prefs::load(blocker.join("settings.json"));
        assert!(p.set("k", Some(json!(1))).is_err());
        // target is a directory -> rename fails
        let target = dir.join("asdir");
        std::fs::create_dir_all(target.join("inner")).unwrap();
        let mut p = Prefs::load(&target);
        assert!(p.set("k", Some(json!(1))).is_err());
        // .new path is a directory -> write fails
        let t2 = dir.join("t2.json");
        std::fs::create_dir_all(dir.join("t2.json.new")).unwrap();
        let mut p = Prefs::load(&t2);
        assert!(p.set("k", Some(json!(1))).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
