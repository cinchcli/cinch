use rusqlite::params;

use super::{Database, SourceAlertSetting, SourceSetting};

impl Database {
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        );
        match result {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("get_setting failed: {}", e)),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| format!("set_setting failed: {}", e))?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM settings WHERE key = ?1", params![key])
            .map_err(|e| format!("delete_setting failed: {}", e))?;
        Ok(())
    }

    pub fn is_source_auto_copy(&self, source: &str) -> Result<bool, String> {
        let key = format!("auto_copy:{}", source);
        match self.get_setting(&key)? {
            Some(val) => Ok(val == "true"),
            None => Ok(false),
        }
    }

    pub fn set_source_auto_copy(&self, source: &str, enabled: bool) -> Result<(), String> {
        let key = format!("auto_copy:{}", source);
        self.set_setting(&key, if enabled { "true" } else { "false" })
    }

    pub fn is_source_alert_enabled(&self, source: &str) -> Result<bool, String> {
        let key = format!("alert_enabled:{}", source);
        match self.get_setting(&key)? {
            Some(val) => Ok(val == "true"),
            None => Ok(true),
        }
    }

    pub fn set_source_alert_enabled(&self, source: &str, enabled: bool) -> Result<(), String> {
        let key = format!("alert_enabled:{}", source);
        self.set_setting(&key, if enabled { "true" } else { "false" })
    }

    /// Returns true if this source has never had an auto_copy setting saved.
    #[cfg(test)]
    pub fn is_source_new(&self, source: &str) -> Result<bool, String> {
        let key = format!("auto_copy:{}", source);
        Ok(self.get_setting(&key)?.is_none())
    }

    /// Returns auto_copy status for all known sources.
    pub fn get_all_source_settings(&self) -> Result<Vec<SourceSetting>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings WHERE key LIKE 'auto_copy:%'")
            .map_err(|e| format!("prepare failed: {}", e))?;

        let settings = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let value: String = row.get(1)?;
                let source = key.strip_prefix("auto_copy:").unwrap_or(&key).to_string();
                Ok(SourceSetting {
                    source,
                    auto_copy: value == "true",
                })
            })
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(settings)
    }

    pub fn get_all_source_alert_settings(&self) -> Result<Vec<SourceAlertSetting>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings WHERE key LIKE 'alert_enabled:%'")
            .map_err(|e| format!("prepare failed: {}", e))?;

        let settings = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let value: String = row.get(1)?;
                let source = key
                    .strip_prefix("alert_enabled:")
                    .unwrap_or(&key)
                    .to_string();
                Ok(SourceAlertSetting {
                    source,
                    alert_enabled: value == "true",
                })
            })
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::test_db;

    #[test]
    fn test_delete_setting() {
        let db = test_db();
        db.set_setting("foo", "bar").unwrap();
        assert_eq!(db.get_setting("foo").unwrap(), Some("bar".to_string()));
        db.delete_setting("foo").unwrap();
        assert_eq!(db.get_setting("foo").unwrap(), None);
        // deleting a non-existent key is a no-op, not an error
        db.delete_setting("foo").unwrap();
    }

    #[test]
    fn test_settings_crud() {
        let db = test_db();

        // No setting yet
        assert_eq!(db.get_setting("foo").unwrap(), None);

        // Set and get
        db.set_setting("foo", "bar").unwrap();
        assert_eq!(db.get_setting("foo").unwrap(), Some("bar".to_string()));

        // Overwrite
        db.set_setting("foo", "baz").unwrap();
        assert_eq!(db.get_setting("foo").unwrap(), Some("baz".to_string()));
    }

    #[test]
    fn test_source_auto_copy() {
        let db = test_db();

        // New source has no setting
        assert!(db.is_source_new("remote:prod").unwrap());
        assert!(!db.is_source_auto_copy("remote:prod").unwrap());

        // Enable auto_copy
        db.set_source_auto_copy("remote:prod", true).unwrap();
        assert!(!db.is_source_new("remote:prod").unwrap());
        assert!(db.is_source_auto_copy("remote:prod").unwrap());

        // Disable auto_copy
        db.set_source_auto_copy("remote:prod", false).unwrap();
        assert!(!db.is_source_auto_copy("remote:prod").unwrap());

        // get_all_source_settings
        db.set_source_auto_copy("remote:staging", true).unwrap();
        let settings = db.get_all_source_settings().unwrap();
        assert_eq!(settings.len(), 2);
    }

    #[test]
    fn test_source_alert_enabled_defaults_on_and_can_be_disabled() {
        let db = test_db();

        assert!(db.is_source_alert_enabled("remote:prod").unwrap());

        db.set_source_alert_enabled("remote:prod", false).unwrap();
        assert!(!db.is_source_alert_enabled("remote:prod").unwrap());

        db.set_source_alert_enabled("remote:prod", true).unwrap();
        assert!(db.is_source_alert_enabled("remote:prod").unwrap());

        let settings = db.get_all_source_alert_settings().unwrap();
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].source, "remote:prod");
        assert!(settings[0].alert_enabled);
    }
}
