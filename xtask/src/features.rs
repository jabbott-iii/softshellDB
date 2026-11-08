// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! `cargo xtask features`: `docs/features.jsonc` agrees with the roadmap.
//!
//! Every ticked `M` task of `docs/roadmap.md` belongs to some feature's
//! `tasks`, and each feature's `ticked` is exactly the roadmap-ticked
//! subset of its `tasks` (left out when there are none). The file's shape
//! is checked too: each feature has a non-empty `name`, `area` and
//! `summary`, `tasks` that the roadmap has, optionally a non-empty
//! `ticked`, and no other keys. Areas are not checked against the list in
//! the file's header comment, which may grow. A roadmap line that looks like a task but does not
//! parse as one is a finding, so that a task is never skipped unseen.

use std::collections::BTreeSet;

use softshell_codec::jsonc;
use softshell_codec::value::{Object, Value};
use softshell_core::limits::Limits;

use crate::check::Finding;
use crate::error::XtaskError;
use crate::workspace::Workspace;

/// The roadmap.
pub(crate) const ROADMAP: &str = "docs/roadmap.md";

/// The feature list.
pub(crate) const FEATURES: &str = "docs/features.jsonc";

/// One roadmap task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Task {
    /// Its ID, such as `M0.6`.
    pub(crate) id: String,
    /// Whether its box is ticked.
    pub(crate) ticked: bool,
}

/// Runs the check.
///
/// # Errors
///
/// [`XtaskError`] if either file cannot be read, or the feature list is not
/// JSONC.
pub(crate) fn run(workspace: &Workspace) -> Result<Vec<Finding>, XtaskError> {
    let roadmap = workspace.read_text(ROADMAP)?;
    let features = workspace.read_text(FEATURES)?;
    let (tasks, mut findings) = parse_roadmap(&roadmap);
    let list = jsonc::parse(&features, Limits::default()).map_err(|source| XtaskError::Json {
        what: FEATURES.to_owned(),
        source,
    })?;
    findings.extend(check(&tasks, &list));
    Ok(findings)
}

/// The roadmap's tasks: lines `- [ ] **ID — title**` and `- [x] **ID — …`,
/// and a finding for each ID given twice and each checkbox line that does
/// not parse.
pub(crate) fn parse_roadmap(text: &str) -> (Vec<Task>, Vec<Finding>) {
    let mut tasks: Vec<Task> = Vec::new();
    let mut findings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim_start();
        if !looks_like_a_task(line) {
            continue;
        }
        let problem = |message: String| Finding::at_line(ROADMAP, index + 1, message);
        let parsed = line
            .strip_prefix("- [ ] **")
            .map(|rest| (false, rest))
            .or_else(|| line.strip_prefix("- [x] **").map(|rest| (true, rest)))
            .and_then(|(ticked, rest)| {
                let (id, _) = rest.split_once(" — ")?;
                is_task_id(id).then_some((ticked, id))
            });
        let Some((ticked, id)) = parsed else {
            findings.push(problem(
                "looks like a task but does not parse: write `- [ ] **ID — title**` or `- [x] **ID — title**`".to_owned(),
            ));
            continue;
        };
        if tasks.iter().any(|t| t.id == id) {
            findings.push(problem(format!("task {id} appears twice")));
            continue;
        }
        tasks.push(Task {
            id: id.to_owned(),
            ticked,
        });
    }
    (tasks, findings)
}

/// A list item with a checkbox, `- [?]`, `* [?]` or `+ [?]`, of any mark.
fn looks_like_a_task(line: &str) -> bool {
    let mut chars = line.chars();
    matches!(chars.next(), Some('-' | '*' | '+'))
        && chars.next().is_some_and(char::is_whitespace)
        && chars.as_str().trim_start().starts_with('[')
        && chars.as_str().trim_start().chars().nth(2) == Some(']')
}

/// A letter, then `<digits>.<digits>`: `D0.1`, `M12.2`.
fn is_task_id(text: &str) -> bool {
    let mut chars = text.chars();
    let letter = chars.next().is_some_and(|c| c.is_ascii_uppercase());
    let rest = chars.as_str();
    let numbers: Vec<&str> = rest.split('.').collect();
    letter
        && numbers.len() == 2
        && numbers
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The findings for the parsed feature list `list` against `tasks`.
pub(crate) fn check(tasks: &[Task], list: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut problem = |message: String| findings.push(Finding::new(FEATURES, message));
    let features = match list.as_object().map(|o| (o, o.get("features"))) {
        Some((root, Some(Value::Array(features)))) if root.len() == 1 => features,
        _ => {
            problem("the top level must be an object with only `features`, an array".to_owned());
            return findings;
        }
    };
    let roadmap_ticked: BTreeSet<&str> = tasks
        .iter()
        .filter(|t| t.ticked)
        .map(|t| t.id.as_str())
        .collect();
    let mut covered = BTreeSet::new();
    for (index, feature) in features.iter().enumerate() {
        let Some(feature) = feature.as_object() else {
            problem(format!("feature {} is not an object", index + 1));
            continue;
        };
        let name = match feature.get("name").and_then(Value::as_str) {
            Some(name) if !name.trim().is_empty() => name.to_owned(),
            _ => format!("feature {}", index + 1),
        };
        for message in check_feature(&name, feature, tasks) {
            problem(message);
        }
        let ids = |key: &str| -> BTreeSet<String> {
            feature
                .get(key)
                .and_then(Value::as_array)
                .unwrap_or(&[])
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        };
        let listed = ids("tasks");
        let ticked = ids("ticked");
        let expected: BTreeSet<String> = listed
            .iter()
            .filter(|id| roadmap_ticked.contains(id.as_str()))
            .cloned()
            .collect();
        if ticked != expected {
            let missing: Vec<&str> = expected.difference(&ticked).map(String::as_str).collect();
            let extra: Vec<&str> = ticked.difference(&expected).map(String::as_str).collect();
            let mut message = format!("`{name}`: `ticked` does not match the roadmap");
            if !missing.is_empty() {
                message.push_str(&format!("; the roadmap ticks {}", missing.join(", ")));
            }
            if !extra.is_empty() {
                message.push_str(&format!("; it does not tick {}", extra.join(", ")));
            }
            message.push_str(" (rewrite the summary for what is built, then update `ticked`)");
            problem(message);
        }
        covered.extend(listed);
    }
    for id in &roadmap_ticked {
        if id.starts_with('M') && !covered.contains(*id) {
            problem(format!("ticked task {id} belongs to no feature"));
        }
    }
    findings
}

/// The problems with one feature's fields.
fn check_feature(name: &str, feature: &Object, tasks: &[Task]) -> Vec<String> {
    let mut problems = Vec::new();
    for key in feature.keys() {
        if !["name", "area", "summary", "tasks", "ticked"].contains(&key) {
            problems.push(format!("`{name}` has an unknown key `{key}`"));
        }
    }
    for key in ["name", "area", "summary"] {
        if !feature
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
        {
            problems.push(format!("`{name}` needs `{key}`, a non-empty string"));
        }
    }
    let listed = id_list(name, feature, "tasks", tasks, &mut problems);
    match &listed {
        Some(ids) if ids.is_empty() => problems.push(format!("`{name}` has no `tasks`")),
        None if feature.get("tasks").is_none() => problems.push(format!("`{name}` needs `tasks`")),
        _ => {}
    }
    if let Some(ticked) = id_list(name, feature, "ticked", tasks, &mut problems) {
        if ticked.is_empty() {
            problems.push(format!(
                "`{name}` has an empty `ticked`; leave it out instead"
            ));
        }
        for id in &ticked {
            if listed.as_ref().is_some_and(|l| !l.contains(id)) {
                problems.push(format!("`{name}` ticks {id}, which is not in its `tasks`"));
            }
        }
    }
    problems
}

/// The IDs in `feature[key]`, if it is there: an array of unique strings,
/// each a roadmap task.
fn id_list(
    name: &str,
    feature: &Object,
    key: &str,
    tasks: &[Task],
    problems: &mut Vec<String>,
) -> Option<Vec<String>> {
    let values = feature.get(key)?;
    let Some(values) = values.as_array() else {
        problems.push(format!("`{name}`: `{key}` must be an array of task IDs"));
        return None;
    };
    let mut ids: Vec<String> = Vec::new();
    for value in values {
        let Some(id) = value.as_str() else {
            problems.push(format!("`{name}`: `{key}` must hold only strings"));
            continue;
        };
        if ids.iter().any(|seen| seen == id) {
            problems.push(format!("`{name}`: `{key}` lists {id} twice"));
        } else if !tasks.iter().any(|t| t.id == id) {
            problems.push(format!(
                "`{name}`: `{key}` lists {id}, which the roadmap does not have"
            ));
        }
        ids.push(id.to_owned());
    }
    Some(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROADMAP_TEXT: &str = "\
- [x] **D0.1 — Review**
- [x] **M0.1 — Skeleton**
  - Depends: D0.1
- [x] **M0.2 — Bytes**
- [ ] **M0.3 — Hashes**
Some prose.
- [x] **M1.1 — Files**
";

    fn findings(features: &str) -> Vec<String> {
        let (tasks, mut found) = parse_roadmap(ROADMAP_TEXT);
        let list = jsonc::parse(features, Limits::default()).unwrap();
        found.extend(check(&tasks, &list));
        found.iter().map(|f| f.message.clone()).collect()
    }

    #[test]
    fn roadmap_lines() {
        let (tasks, found) = parse_roadmap(ROADMAP_TEXT);
        let ids: Vec<_> = tasks.iter().map(|t| (t.id.as_str(), t.ticked)).collect();
        assert_eq!(
            ids,
            [
                ("D0.1", true),
                ("M0.1", true),
                ("M0.2", true),
                ("M0.3", false),
                ("M1.1", true)
            ]
        );
        assert_eq!(found, []);
        let (crlf, found) = parse_roadmap(&ROADMAP_TEXT.replace('\n', "\r\n"));
        assert_eq!((crlf, found), (tasks, vec![]));
        let (_, found) = parse_roadmap("- [ ] **M0.1 — a**\n- [x] **M0.1 — b**\n");
        assert_eq!(
            found[0].to_string(),
            "docs/roadmap.md:2: task M0.1 appears twice"
        );
        // Lines that look like tasks but would be skipped are findings.
        let near_misses = "\
- [X] **M2.1 — capital X**
- [x] **M2.2 - a hyphen**
* [ ] **M2.3 — another bullet**
  - [x] **M2.4** — the dash outside
- [ ] A checklist item
-  [ ] **M2.5 — two spaces**
";
        let (tasks, found) = parse_roadmap(near_misses);
        assert_eq!(tasks, []);
        let lines: Vec<_> = found.iter().map(|f| f.location.as_str()).collect();
        assert_eq!(
            lines,
            [1, 2, 3, 4, 5, 6].map(|l| format!("docs/roadmap.md:{l}"))
        );
        assert!(
            found[0]
                .message
                .starts_with("looks like a task but does not parse")
        );
        let (_, found) = parse_roadmap("- item\n- [link](x)\n* plain\n");
        assert_eq!(found, []);
        for id in ["M0.1", "D0.1", "M12.20"] {
            assert!(is_task_id(id), "{id}");
        }
        for id in ["", "M", "M0", "m0.1", "M0.1.2", "M.1", "MX0.1", "M0.x"] {
            assert!(!is_task_id(id), "{id}");
        }
    }

    #[test]
    fn a_matching_list_passes() {
        let text = r#"{ "features": [
            { "name": "A", "area": "Quality", "summary": "s.", "tasks": ["M0.1", "M0.3"], "ticked": ["M0.1"] },
            // Comments and trailing commas: it is JSONC.
            { "name": "B", "area": "Storage", "summary": "s.", "tasks": ["M0.2", "M1.1"], "ticked": ["M1.1", "M0.2"], },
        ] }"#;
        assert_eq!(findings(text), Vec::<String>::new());
    }

    #[test]
    fn disagreements_with_the_roadmap_are_found() {
        let text = r#"{ "features": [
            { "name": "A", "area": "Quality", "summary": "s.", "tasks": ["M0.1", "M0.3"], "ticked": ["M0.3"] },
            { "name": "B", "area": "Storage", "summary": "s.", "tasks": ["M0.2", "M0.3"] },
        ] }"#;
        assert_eq!(
            findings(text),
            [
                "`A`: `ticked` does not match the roadmap; the roadmap ticks M0.1; it does not tick M0.3 (rewrite the summary for what is built, then update `ticked`)",
                "`B`: `ticked` does not match the roadmap; the roadmap ticks M0.2 (rewrite the summary for what is built, then update `ticked`)",
                "ticked task M1.1 belongs to no feature",
            ]
        );
    }

    #[test]
    fn malformed_features_are_found() {
        let text = r#"{ "features": [
            { "name": "A", "area": "", "summary": "", "tasks": ["M0.1", "M0.1", "M9.9"], "ticked": ["M0.1", "M0.2"], "notes": 1 },
            { "name": "B", "area": "Storage", "summary": "s.", "tasks": [], "ticked": [] },
            { "area": "Storage", "summary": "s.", "tasks": "M1.1" },
            3,
        ] }"#;
        let found = findings(text);
        for expected in [
            "`A` has an unknown key `notes`",
            "`A` needs `summary`, a non-empty string",
            "`A` needs `area`, a non-empty string",
            "`A`: `tasks` lists M0.1 twice",
            "`A`: `tasks` lists M9.9, which the roadmap does not have",
            "`A` ticks M0.2, which is not in its `tasks`",
            "`B` has no `tasks`",
            "`B` has an empty `ticked`; leave it out instead",
            "`feature 3` needs `name`, a non-empty string",
            "`feature 3`: `tasks` must be an array of task IDs",
            "feature 4 is not an object",
            "ticked task M1.1 belongs to no feature",
        ] {
            assert!(
                found.iter().any(|f| f.starts_with(expected)),
                "{expected}\n{found:#?}"
            );
        }
        assert_eq!(
            findings(r#"{"features": [], "extra": 1}"#),
            ["the top level must be an object with only `features`, an array"]
        );
    }
}
