//! Custom fields and custom source types, defined by the person and shared as a file.
//!
//! A custom field holds a typed value (text, number, date, list, yes/no or link) with an optional
//! default, a required flag and, for lists, the allowed values. Values live in each record's
//! `custom` map under the field's id, which is a stable slug of its name.
//!
//! A custom type is a named variant of a built-in type ("Policy brief" based on Report): it has
//! its own label, field set and help, while citation styles and exchange formats use the base
//! type, so output stays correct.

use super::Library;
use crate::error::{Error, Result};
use crate::model::{CustomFieldDef, CustomType, Reference};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const KINDS: &[&str] = &["text", "number", "date", "list", "boolean", "link"];
pub const DEFINITIONS_FORMAT: &str = "prism-reference-manager/custom-definitions";

fn slug(s: &str) -> String {
    let f = crate::util::fold(s);
    let mut out = String::new();
    for c in f.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').chars().take(40).collect()
}

/// Read a number written with either convention: "1,250" and "1,250.5" use commas between
/// thousands; "2,5" uses a decimal comma; spaces between digit groups are ignored.
fn parse_number(s: &str) -> Option<f64> {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let thousands = re!(r"^-?\d{1,3}(,\d{3})+(\.\d+)?$").is_match(&t);
    let n = if thousands { t.replace(',', "") } else if t.contains(',') && !t.contains('.') { t.replace(',', ".") } else { t };
    n.parse::<f64>().ok().filter(|x| x.is_finite())
}

/// Check one value against its field definition; returns the value in canonical form.
pub fn check_value(def: &CustomFieldDef, v: &Value) -> std::result::Result<Value, String> {
    let s = match v {
        Value::String(s) => s.trim().to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    if s.is_empty() {
        return if def.required { Err(format!("“{}” is required.", def.name)) } else { Ok(Value::Null) };
    }
    match def.kind.as_str() {
        "number" => parse_number(&s).map(|n| json!(n)).ok_or_else(|| format!("“{}” must be a number.", def.name)),
        "date" => {
            let ok = re!(r"^\d{4}(-(0[1-9]|1[0-2])(-(0[1-9]|[12]\d|3[01]))?)?$").is_match(&s);
            if ok { Ok(json!(s)) } else { Err(format!("“{}” must be a date such as 2024, 2024-03 or 2024-03-15.", def.name)) }
        }
        "list" => {
            if def.options.iter().any(|o| o == &s) { Ok(json!(s)) } else { Err(format!("“{}” must be one of: {}.", def.name, def.options.join(", "))) }
        }
        "boolean" => match s.to_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(json!(true)),
            "false" | "no" | "0" => Ok(json!(false)),
            _ => Err(format!("“{}” must be yes or no.", def.name)),
        },
        "link" => {
            if s.starts_with("https://") || s.starts_with("http://") { Ok(json!(s)) } else { Err(format!("“{}” must be a web address starting with https://.", def.name)) }
        }
        _ => Ok(json!(s)),
    }
}

impl Library {
    pub fn custom_fields(&self) -> Result<Vec<CustomFieldDef>> {
        let mut v: Vec<CustomFieldDef> = self.store.all()?;
        v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(v)
    }

    pub fn custom_types(&self) -> Result<Vec<CustomType>> {
        let mut v: Vec<CustomType> = self.store.all()?;
        v.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
        Ok(v)
    }

    /// Create or change a custom field. The id is fixed when the field is created.
    pub fn save_custom_field(&mut self, mut d: CustomFieldDef) -> Result<CustomFieldDef> {
        d.name = d.name.trim().to_string();
        if d.name.is_empty() {
            return Err(Error::user("Give the field a name."));
        }
        if !KINDS.contains(&d.kind.as_str()) {
            return Err(Error::user("Choose what kind of value the field holds."));
        }
        d.options = d.options.iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
        d.options.dedup();
        if d.kind == "list" && d.options.is_empty() {
            return Err(Error::user("A list field needs at least one allowed value."));
        }
        let all = self.custom_fields()?;
        if all.iter().any(|x| x.id != d.id && x.name.to_lowercase() == d.name.to_lowercase()) {
            return Err(Error::user(&format!("A field called “{}” already exists.", d.name)));
        }
        if d.id.is_empty() {
            let base = slug(&d.name);
            let base = if base.is_empty() { "field".to_string() } else { base };
            let mut id = base.clone();
            let mut n = 2;
            while all.iter().any(|x| x.id == id) || crate::types::COMMON_FIELDS.contains(&id.as_str()) {
                id = format!("{base}_{n}");
                n += 1;
            }
            d.id = id;
        }
        if !d.default.trim().is_empty() {
            check_value(&CustomFieldDef { required: false, ..d.clone() }, &json!(d.default)).map_err(|e| Error::user(&format!("The default value does not fit: {e}")))?;
        }
        let dd = d.clone();
        self.write(&format!("Save field “{}”", d.name), |tx| tx.put(&dd))?;
        Ok(d)
    }

    /// Delete a custom field. With `remove_values`, its values are removed from every source in
    /// the same step; otherwise deletion is refused while sources have values.
    pub fn delete_custom_field(&mut self, id: &str, remove_values: bool) -> Result<usize> {
        let d = self.store.must::<CustomFieldDef>(id)?;
        let path = format!("$.custom.\"{}\"", id.replace('"', ""));
        let holders: Vec<String> = {
            let mut st = self.store.conn.prepare("SELECT id FROM refs WHERE json_extract(data, ?1) IS NOT NULL")?;
            let v = st.query_map([&path], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).collect();
            v
        };
        if !holders.is_empty() && !remove_values {
            return Err(Error::user(&format!("{} sources have a value in “{}”. Delete the field together with those values, or keep it.", holders.len(), d.name)));
        }
        let n = holders.len();
        let idc = id.to_string();
        self.write(&format!("Delete field “{}”", d.name), |tx| {
            for h in &holders {
                let mut r = tx.must::<Reference>(h)?;
                r.custom.remove(&idc);
                r.revision += 1;
                tx.put(&r)?;
            }
            tx.delete::<CustomFieldDef>(&idc)
        })?;
        Ok(n)
    }

    pub fn save_custom_type(&mut self, mut t: CustomType) -> Result<CustomType> {
        t.label = t.label.trim().to_string();
        if t.label.is_empty() {
            return Err(Error::user("Give the type a name."));
        }
        if crate::types::get(&t.base).is_none() {
            return Err(Error::user("Choose the built-in type it is based on; citation styles format it like that type."));
        }
        let all = self.custom_types()?;
        if all.iter().any(|x| x.id != t.id && x.label.to_lowercase() == t.label.to_lowercase()) || crate::types::TYPES.iter().any(|b| b.label.to_lowercase() == t.label.to_lowercase()) {
            return Err(Error::user(&format!("A type called “{}” already exists.", t.label)));
        }
        if t.id.is_empty() {
            let base = format!("custom-{}", slug(&t.label).replace('_', "-"));
            let mut id = base.clone();
            let mut n = 2;
            while all.iter().any(|x| x.id == id) {
                id = format!("{base}-{n}");
                n += 1;
            }
            t.id = id;
        }
        if t.fields.is_empty() {
            t.fields = crate::types::get(&t.base).map(|b| b.fields.iter().map(|f| f.to_string()).collect()).unwrap_or_default();
        }
        let tt = t.clone();
        self.write(&format!("Save type “{}”", t.label), |tx| tx.put(&tt))?;
        Ok(t)
    }

    /// Delete a custom type. Sources of that type keep their base type.
    pub fn delete_custom_type(&mut self, id: &str) -> Result<usize> {
        let t = self.store.must::<CustomType>(id)?;
        let holders: Vec<String> = {
            let mut st = self.store.conn.prepare("SELECT id FROM refs WHERE json_extract(data,'$.subtype')=?1")?;
            let v = st.query_map([id], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).collect();
            v
        };
        let n = holders.len();
        let idc = id.to_string();
        self.write(&format!("Delete type “{}”", t.label), |tx| {
            for h in &holders {
                let mut r = tx.must::<Reference>(h)?;
                r.subtype.clear();
                r.revision += 1;
                tx.put(&r)?;
            }
            tx.delete::<CustomType>(&idc)
        })?;
        Ok(n)
    }

    /// Fill defaults for fields that apply to this source and have none yet.
    pub fn apply_custom_defaults(&self, r: &mut Reference) -> Result<()> {
        for d in self.custom_fields()? {
            if d.default.trim().is_empty() || r.custom.contains_key(&d.id) || !applies(&d, r) {
                continue;
            }
            if let Ok(v) = check_value(&CustomFieldDef { required: false, ..d.clone() }, &json!(d.default)) {
                if !v.is_null() {
                    r.custom.insert(d.id.clone(), v);
                }
            }
        }
        Ok(())
    }

    /// Validate custom values; values are stored in canonical form. Unknown keys are kept.
    pub fn check_custom(&self, r: &mut Reference) -> Result<()> {
        let defs = self.custom_fields()?;
        let mut problems = vec![];
        for d in &defs {
            if !applies(d, r) {
                continue;
            }
            let v = r.custom.get(&d.id).cloned().unwrap_or(Value::Null);
            match check_value(d, &v) {
                Ok(Value::Null) => {
                    r.custom.remove(&d.id);
                }
                Ok(n) => {
                    r.custom.insert(d.id.clone(), n);
                }
                Err(e) => problems.push(e),
            }
        }
        if problems.is_empty() { Ok(()) } else { Err(Error::user(&problems.join(" "))) }
    }

    /// Values already used in a field, most common first, for suggestions while typing.
    pub fn field_values(&self, field: &str, prefix: &str) -> Result<Vec<String>> {
        let path = if let Some(c) = field.strip_prefix("custom:") { format!("$.custom.\"{}\"", c.replace('"', "")) } else { format!("$.fields.{}", field.replace(|c: char| !c.is_ascii_alphanumeric() && c != '_', "")) };
        let mut st = self.store.conn.prepare("SELECT v, COUNT(*) n FROM (SELECT json_extract(data, ?1) v FROM refs WHERE trashed IS NULL) WHERE v IS NOT NULL AND v != '' AND lower(v) LIKE lower(?2) GROUP BY v ORDER BY n DESC, v LIMIT 15")?;
        let like = format!("{}%", prefix.replace(['%', '_'], ""));
        let v = st.query_map(rusqlite::params![path, like], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).collect();
        Ok(v)
    }

    /// The shareable definitions file.
    pub fn export_definitions(&self) -> Result<Value> {
        Ok(json!({ "format": DEFINITIONS_FORMAT, "version": 1, "exported": crate::util::now(), "fields": self.custom_fields()?, "types": self.custom_types()? }))
    }

    /// Merge a definitions file: fields match by id, then by name; types by id, then by label.
    pub fn import_definitions(&mut self, text: &str) -> Result<Value> {
        let v: Value = serde_json::from_str(text).map_err(|_| Error::user("This is not a definitions file (it is not valid JSON)."))?;
        if v["format"].as_str() != Some(DEFINITIONS_FORMAT) {
            return Err(Error::user("This file is not a Prism Reference Manager definitions file."));
        }
        let fields: Vec<CustomFieldDef> = serde_json::from_value(v["fields"].clone()).unwrap_or_default();
        let types: Vec<CustomType> = serde_json::from_value(v["types"].clone()).unwrap_or_default();
        let have_f = self.custom_fields()?;
        let have_t = self.custom_types()?;
        let (mut fa, mut fu, mut ta, mut tu, mut problems) = (0, 0, 0, 0, vec![]);
        for mut f in fields {
            if let Some(e) = have_f.iter().find(|x| x.id == f.id || x.name.to_lowercase() == f.name.to_lowercase()) {
                f.id = e.id.clone();
                fu += 1;
            } else {
                // Keep the shared id so values exchanged between libraries line up.
                if have_f.iter().any(|x| x.id == f.id) {
                    f.id.clear();
                }
                fa += 1;
            }
            if let Err(e) = self.save_custom_field(f.clone()) {
                problems.push(format!("{}: {e}", f.name));
            }
        }
        for mut t in types {
            if let Some(e) = have_t.iter().find(|x| x.id == t.id || x.label.to_lowercase() == t.label.to_lowercase()) {
                t.id = e.id.clone();
                tu += 1;
            } else {
                ta += 1;
            }
            if let Err(e) = self.save_custom_type(t.clone()) {
                problems.push(format!("{}: {e}", t.label));
            }
        }
        Ok(json!({ "fields_added": fa, "fields_updated": fu, "types_added": ta, "types_updated": tu, "problems": problems }))
    }
}

/// Whether a field applies to a source (no types listed means every type).
pub fn applies(d: &CustomFieldDef, r: &Reference) -> bool {
    d.types.is_empty() || d.types.iter().any(|t| t == &r.rtype || (!r.subtype.is_empty() && t == &r.subtype))
}

/// Display label for a source's type, using its custom type when it has one.
pub fn type_label(r: &Reference, custom: &BTreeMap<String, String>) -> String {
    if !r.subtype.is_empty() {
        if let Some(l) = custom.get(&r.subtype) {
            return l.clone();
        }
    }
    crate::types::label(&r.rtype).to_string()
}
