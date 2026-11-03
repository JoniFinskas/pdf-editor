use super::*;
use serde_json::{Value, json};
use std::io::Write;

const LIMIT: usize = 512_000;
const FIELDS: &[&str] = &["name", "headline", "contact", "sections", "letter"];

impl CvSpec {
    // Drafts may be unfinished, including a temporarily restored 21st section.
    fn validate_draft_storage(&self) -> Result<()> {
        if self.sections.len() > 21 || self.contact.len() > 100 {
            return Err(err(
                "Draft exceeds the supported number of sections or contact lines",
            ));
        }
        let mut bytes = 0usize;
        for text in std::iter::once(&self.name)
            .chain(std::iter::once(&self.headline))
            .chain(&self.contact)
            .chain(self.sections.iter().flat_map(|s| [&s.heading, &s.body]))
        {
            bytes = bytes.saturating_add(text.len());
            if bytes > 64000
                || text
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            {
                return Err(err(
                    "Draft text exceeds 64 KB or contains unsupported control characters",
                ));
            }
        }
        Ok(())
    }

    /// Serialize an unfinished CV, with an explicit version and no PDF or history data.
    pub fn to_draft_json(&self) -> Result<String> {
        self.validate_draft_storage()?;
        serde_json::to_string_pretty(&json!({"version":1,"cv":{
            "name":self.name,"headline":self.headline,"contact":self.contact,
            "sections":self.sections.iter().map(|s|json!({"heading":s.heading,"body":s.body})).collect::<Vec<_>>(),
            "letter":self.letter
        }})).map_err(|_|err("Could not encode the CV draft"))
    }

    /// Parse bounded, versioned CV data; creating a PDF still requires validate().
    pub fn from_draft_json(text: &str) -> Result<Self> {
        if text.len() > LIMIT {
            return Err(err("CV draft exceeds 512 KB"));
        }
        let value: Value = serde_json::from_str(text).map_err(|_| err("Invalid CV draft JSON"))?;
        let object = value
            .as_object()
            .ok_or_else(|| err("Expected a CV draft object"))?;
        if object.len() != 2
            || value.get("version").and_then(Value::as_u64) != Some(1)
            || !object.contains_key("cv")
        {
            return Err(err("Unsupported CV draft format or version"));
        }
        let cv = &value["cv"];
        if cv
            .as_object()
            .is_none_or(|o| o.keys().any(|key| !FIELDS.contains(&key.as_str())))
        {
            return Err(err("Unknown CV draft field"));
        }
        let spec = Self::from_value(cv)?;
        spec.validate_draft_storage()?;
        Ok(spec)
    }

    /// Write a new .cvdraft copy. Existing files are never overwritten.
    pub fn save_draft(&self, path: &Path) -> Result<()> {
        draft_extension(path)?;
        let text = self.to_draft_json()?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    err("Draft already exists. Choose a new filename.")
                } else {
                    error.into()
                }
            })?;
        if let Err(error) = file
            .write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
        {
            drop(file);
            let _ = fs::remove_file(path);
            return Err(error.into());
        }
        Ok(())
    }

    /// Read an explicitly selected regular .cvdraft file, limited to 512 KB.
    pub fn load_draft(path: &Path) -> Result<Self> {
        draft_extension(path)?;
        if !fs::metadata(path)?.is_file() {
            return Err(err("Choose a regular CV draft file"));
        }
        let bytes = read_limited(path, LIMIT)?;
        let text =
            std::str::from_utf8(&bytes).map_err(|_| err("CV draft must contain UTF-8 JSON"))?;
        Self::from_draft_json(text)
    }

    pub(crate) fn from_value(value: &Value) -> Result<Self> {
        fn text(value: &Value, key: &str, default: Option<&str>) -> Result<String> {
            match value.get(key) {
                Some(Value::String(text)) => Ok(text.clone()),
                None => default
                    .map(str::to_owned)
                    .ok_or_else(|| err(&format!("Expected string: {key}"))),
                _ => Err(err(&format!("Expected string: {key}"))),
            }
        }
        let sections = value
            .get("sections")
            .and_then(Value::as_array)
            .ok_or_else(|| err("Expected array: sections"))?
            .iter()
            .map(|section| {
                if section
                    .as_object()
                    .is_none_or(|o| o.keys().any(|k| k != "heading" && k != "body"))
                {
                    return Err(err("CV section fields are heading and body"));
                }
                Ok(CvSection {
                    heading: text(section, "heading", None)?,
                    body: text(section, "body", None)?,
                })
            })
            .collect::<Result<_>>()?;
        let contact = match value.get("contact") {
            None => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| err("Expected text array: contact"))
                })
                .collect::<Result<_>>()?,
            _ => return Err(err("Expected array: contact")),
        };
        let letter = match value.get("letter") {
            None => false,
            Some(Value::Bool(value)) => *value,
            _ => return Err(err("Expected boolean: letter")),
        };
        Ok(Self {
            name: text(value, "name", None)?,
            headline: text(value, "headline", Some(""))?,
            contact,
            sections,
            letter,
        })
    }
}

fn draft_extension(path: &Path) -> Result<()> {
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("cvdraft"))
    {
        Ok(())
    } else {
        Err(err("Choose a .cvdraft file"))
    }
}
