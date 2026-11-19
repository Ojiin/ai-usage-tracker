//! Start the tracker when you sign in to Windows, through the per-user Run key (no admin
//! needed). The port of startup.py: the same key, the same value name and the same command
//! line, so an entry made by the Python widget is recognised and follows the exe.
//!
//! Only this app's own value is read or written. If the exe has moved since the option was
//! turned on, launching it from the new place updates the entry. On other systems there is no
//! Run key: the option reads as off and turning it on does nothing.

use std::io;
use std::path::Path;

#[cfg_attr(not(windows), allow(dead_code))] // only the registry reads it
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const NAME: &str = "AIUsageTracker";

/// What Windows should run: the exe itself, in quotes (its folder may have spaces).
pub fn command(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// The place the entry is kept: the registry on Windows, a plain map in tests.
pub trait RunEntries {
    fn get(&self, name: &str) -> Option<String>;
    fn set(&mut self, name: &str, command: &str) -> io::Result<()>;
    /// Removing an entry that is not there is fine.
    fn remove(&mut self, name: &str) -> io::Result<()>;
}

pub fn is_enabled(entries: &dyn RunEntries) -> bool {
    entries.get(NAME).is_some()
}

pub fn enable(entries: &mut dyn RunEntries, exe: &Path) -> io::Result<()> {
    entries.set(NAME, &command(exe))
}

pub fn disable(entries: &mut dyn RunEntries) -> io::Result<()> {
    entries.remove(NAME)
}

/// Keep an enabled entry pointing at this copy of the app; never turns it on.
pub fn refresh(entries: &mut dyn RunEntries, exe: &Path) -> io::Result<()> {
    match entries.get(NAME) {
        Some(current) if current != command(exe) => enable(entries, exe),
        _ => Ok(()),
    }
}

/// The message for the status line when the entry could not be changed.
pub fn failure(error: &io::Error) -> String {
    crate::format::clip(&format!("Couldn't change startup: {error}"), 60)
}

/// This user's Run key.
#[cfg(windows)]
pub struct Registry {
    key: &'static str,
}

#[cfg(windows)]
impl RunEntries for Registry {
    fn get(&self, name: &str) -> Option<String> {
        let root = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        root.open_subkey(self.key).ok()?.get_value(name).ok()
    }

    fn set(&mut self, name: &str, command: &str) -> io::Result<()> {
        let root = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let (key, _) = root.create_subkey(self.key)?;
        key.set_value(name, &command) // a string is written as REG_SZ
    }

    fn remove(&mut self, name: &str) -> io::Result<()> {
        let root = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let removed = root
            .open_subkey_with_flags(self.key, winreg::enums::KEY_SET_VALUE)
            .and_then(|key| key.delete_value(name));
        match removed {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

/// Nothing to write to away from Windows.
#[cfg(not(windows))]
pub struct Nowhere;

#[cfg(not(windows))]
impl RunEntries for Nowhere {
    fn get(&self, _name: &str) -> Option<String> {
        None
    }

    fn set(&mut self, _name: &str, _command: &str) -> io::Result<()> {
        Ok(())
    }

    fn remove(&mut self, _name: &str) -> io::Result<()> {
        Ok(())
    }
}

/// Where this system keeps the entry.
#[cfg(windows)]
pub fn system() -> impl RunEntries {
    Registry { key: RUN_KEY }
}

/// Where this system keeps the entry.
#[cfg(not(windows))]
pub fn system() -> impl RunEntries {
    Nowhere
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[derive(Default)]
    struct Memory(HashMap<String, String>);

    impl RunEntries for Memory {
        fn get(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }

        fn set(&mut self, name: &str, command: &str) -> io::Result<()> {
            self.0.insert(name.into(), command.into());
            Ok(())
        }

        fn remove(&mut self, name: &str) -> io::Result<()> {
            self.0.remove(name);
            Ok(())
        }
    }

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Apps\AIUsageTracker.exe")
    }

    // test_startup.py: test_frozen_exe_runs_itself
    #[test]
    fn the_exe_runs_itself_in_quotes() {
        assert_eq!(command(&exe()), "\"C:\\Apps\\AIUsageTracker.exe\"");
        assert_eq!(
            command(Path::new(r"C:\Program Files\AI Usage\AIUsageTracker.exe")),
            "\"C:\\Program Files\\AI Usage\\AIUsageTracker.exe\""
        );
    }

    #[test]
    fn same_key_and_value_name_as_the_python_widget() {
        assert_eq!(RUN_KEY, "Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        assert_eq!(NAME, "AIUsageTracker");
    }

    // test_startup.py: test_enable_disable_round_trip
    fn round_trip(entries: &mut dyn RunEntries) {
        assert!(!is_enabled(entries));
        enable(entries, &exe()).unwrap();
        assert_eq!(entries.get(NAME), Some(command(&exe())));
        disable(entries).unwrap();
        assert!(!is_enabled(entries));
        disable(entries).unwrap(); // already off is fine
    }

    // test_startup.py: test_refresh_repoints_a_moved_app_but_never_turns_it_on
    fn refresh_follows_the_exe(entries: &mut dyn RunEntries) {
        refresh(entries, &exe()).unwrap();
        assert!(!is_enabled(entries));
        entries
            .set(NAME, "\"C:\\old place\\AIUsageTracker.exe\"")
            .unwrap();
        refresh(entries, &exe()).unwrap();
        assert_eq!(entries.get(NAME), Some(command(&exe())));
        disable(entries).unwrap();
    }

    #[test]
    fn enable_disable_round_trip() {
        round_trip(&mut Memory::default());
    }

    #[test]
    fn refresh_repoints_a_moved_app_but_never_turns_it_on() {
        refresh_follows_the_exe(&mut Memory::default());
    }

    #[test]
    fn failure_message_fits_the_status_line() {
        let error = io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Access is denied. ".repeat(5),
        );
        let message = failure(&error);
        assert!(message.starts_with("Couldn't change startup: Access is denied."));
        assert_eq!(message.chars().count(), 60);
    }

    #[cfg(not(windows))]
    #[test]
    fn away_from_windows_the_option_stays_off() {
        let mut entries = system();
        enable(&mut entries, &exe()).unwrap();
        assert!(!is_enabled(&entries));
        disable(&mut entries).unwrap();
    }

    /// The same two tests on the real registry, in a throwaway key, never the real Run key.
    #[cfg(windows)]
    mod registry {
        use super::*;

        const TEST_KEY: &str = r"Software\AIUsageTrackerTest\Run";

        fn cleanup() {
            let root = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
            let _ = root.delete_subkey_all(r"Software\AIUsageTrackerTest");
        }

        #[test]
        fn registry_round_trip_and_refresh() {
            cleanup();
            round_trip(&mut Registry { key: TEST_KEY });
            refresh_follows_the_exe(&mut Registry { key: TEST_KEY });
            cleanup();
        }
    }
}
