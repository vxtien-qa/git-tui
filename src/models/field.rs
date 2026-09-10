use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// A project field definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub id: String,
    pub name: String,
    pub field_type: FieldType,
    /// For single-select fields, the available options.
    /// For iteration fields, every iteration (active + completed).
    pub options: Vec<FieldOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FieldType {
    Text,
    Number,
    Date,
    SingleSelect,
    Iteration,
    Unknown(String),
}

/// An option for a single-select or iteration field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldOption {
    pub id: String,
    pub name: String,
    /// Iteration start date (iteration fields only) - the authoritative
    /// answer to "which sprint is current", straight from GitHub.
    #[serde(default)]
    pub start_date: Option<NaiveDate>,
    /// Iteration length in days (iteration fields only).
    #[serde(default)]
    pub duration_days: Option<i64>,
}

impl FieldOption {
    /// Whether `date` falls inside this iteration's [start, start + duration) window.
    pub fn contains_date(&self, date: NaiveDate) -> bool {
        match (self.start_date, self.duration_days) {
            (Some(start), Some(days)) if days > 0 => {
                date >= start && (date - start).num_days() < days
            }
            _ => false,
        }
    }
}

impl Field {
    /// Find an option by name (case-insensitive).
    pub fn find_option(&self, name: &str) -> Option<&FieldOption> {
        self.options
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(name))
    }

    /// The iteration containing `date`, if this field carries iteration dates.
    pub fn iteration_for_date(&self, date: NaiveDate) -> Option<&FieldOption> {
        self.options.iter().find(|o| o.contains_date(date))
    }

    /// Whether any option carries iteration dates (i.e. the date-based sprint
    /// lookup can be trusted instead of the hardcoded anchor fallback).
    pub fn has_iteration_dates(&self) -> bool {
        self.options.iter().any(|o| o.start_date.is_some())
    }
}

// ── GraphQL response shapes ───────────────────────────────────
//
// `gh project field-list --format json` does NOT include the `configuration`
// block, so iteration fields came back with ZERO options - which silently
// broke every "inherit the parent's Sprint" write (the option lookup could
// never match). Fields are therefore fetched over GraphQL, which also hands
// us each iteration's startDate/duration.

#[derive(Debug, Deserialize)]
pub struct GhFieldsResponse {
    pub data: Option<GhFieldsData>,
}

#[derive(Debug, Deserialize)]
pub struct GhFieldsData {
    pub organization: Option<GhFieldsOrg>,
}

#[derive(Debug, Deserialize)]
pub struct GhFieldsOrg {
    #[serde(rename = "projectV2")]
    pub project_v2: Option<GhFieldsProject>,
}

#[derive(Debug, Deserialize)]
pub struct GhFieldsProject {
    pub fields: GhFieldsConnection,
}

#[derive(Debug, Deserialize)]
pub struct GhFieldsConnection {
    pub nodes: Vec<GhFieldNode>,
}

#[derive(Debug, Deserialize)]
pub struct GhFieldNode {
    pub id: String,
    pub name: String,
    #[serde(rename = "dataType")]
    pub data_type: Option<String>,
    #[serde(default)]
    pub options: Option<Vec<GhSelectOption>>,
    #[serde(default)]
    pub configuration: Option<GhIterationConfiguration>,
}

#[derive(Debug, Deserialize)]
pub struct GhSelectOption {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct GhIterationConfiguration {
    #[serde(default)]
    pub iterations: Vec<GhIteration>,
    #[serde(rename = "completedIterations", default)]
    pub completed_iterations: Vec<GhIteration>,
}

#[derive(Debug, Deserialize)]
pub struct GhIteration {
    pub id: String,
    pub title: String,
    #[serde(rename = "startDate")]
    pub start_date: Option<NaiveDate>,
    pub duration: Option<i64>,
}

impl GhFieldNode {
    pub fn into_field(self) -> Field {
        let field_type = match self.data_type.as_deref() {
            Some("TEXT") => FieldType::Text,
            Some("NUMBER") => FieldType::Number,
            Some("DATE") => FieldType::Date,
            Some("SINGLE_SELECT") => FieldType::SingleSelect,
            Some("ITERATION") => FieldType::Iteration,
            Some(other) => FieldType::Unknown(other.to_string()),
            None => FieldType::Unknown(String::new()),
        };

        let mut options: Vec<FieldOption> = self
            .options
            .unwrap_or_default()
            .into_iter()
            .map(|o| FieldOption {
                id: o.id,
                name: o.name,
                start_date: None,
                duration_days: None,
            })
            .collect();

        if let Some(config) = self.configuration {
            // Active iterations first so the newest sprints win any ambiguity,
            // then completed ones (needed: reports and sprint filters still
            // reference sprints that have already closed).
            for it in config
                .iterations
                .into_iter()
                .chain(config.completed_iterations)
            {
                options.push(FieldOption {
                    id: it.id,
                    name: it.title,
                    start_date: it.start_date,
                    duration_days: it.duration,
                });
            }
        }

        Field {
            id: self.id,
            name: self.name,
            field_type,
            options,
        }
    }
}
