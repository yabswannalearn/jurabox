//! Small status replies and bounded append/trim patches for visible output.
use super::*;
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize)]
pub struct LogPatch {
    pub project: Uuid,
    pub workload: Uuid,
    pub reset: bool,
    pub trim: usize,
    pub header: String,
    pub text: String,
}

#[derive(Default)]
pub(super) struct Updates {
    selected: Option<Uuid>,
    metadata: Option<Summary>,
    output: HashMap<(Uuid, Uuid), String>,
}

fn parts(text: &str) -> (&str, &str) {
    if (text.starts_with("[Earlier output omitted:")
        || text.starts_with("[Earlier Compose output omitted]"))
        && let Some(end) = text.find('\n')
    {
        return text.split_at(end + 1);
    }
    ("", text)
}

fn overlap(old: &str, new: &str) -> usize {
    let pattern = new.as_bytes();
    if pattern.is_empty() {
        return 0;
    }
    let mut prefix = vec![0; pattern.len()];
    let mut matched = 0;
    for index in 1..pattern.len() {
        while matched > 0 && pattern[index] != pattern[matched] {
            matched = prefix[matched - 1];
        }
        if pattern[index] == pattern[matched] {
            matched += 1;
        }
        prefix[index] = matched;
    }
    matched = 0;
    for byte in old.bytes() {
        while matched > 0 && (matched == pattern.len() || byte != pattern[matched]) {
            matched = prefix[matched - 1];
        }
        if byte == pattern[matched] {
            matched += 1;
        }
    }
    matched
}

impl Updates {
    pub fn prepare(&mut self, response: &mut Response, selected: Option<Uuid>, force: bool) {
        if force || self.selected != selected {
            self.output.clear();
            self.metadata = None;
            self.selected = selected;
        }
        let mut budget = 2 * 1024 * 1024;
        for (project, view) in &mut response.summary.workloads {
            if Some(*project) == selected {
                let key = (*project, view.id);
                let old = self.output.get(&key);
                if old != Some(&view.output) {
                    let (header, text) = parts(&view.output);
                    let body = old.map(|old| parts(old).1).unwrap_or_default();
                    let common = overlap(body, text);
                    if header.len() + text.len() - common > budget {
                        view.output.clear();
                        continue;
                    }
                    budget -= header.len() + text.len() - common;
                    response.logs.push(LogPatch {
                        project: *project,
                        workload: view.id,
                        reset: old.is_none() || common == 0,
                        trim: body.len() - common,
                        header: header.into(),
                        text: text[common..].into(),
                    });
                    self.output.insert(key, view.output.clone());
                }
            }
            view.output.clear();
        }
        response.unchanged = self.metadata.as_ref() == Some(&response.summary)
            && response.logs.is_empty()
            && !response.quit;
        if response.unchanged {
            response.summary = Summary::default();
        } else {
            self.metadata = Some(response.summary.clone());
        }
    }
}

impl Response {
    pub fn apply_logs(&mut self, previous: Option<&Summary>) -> Result<()> {
        for (project, view) in &mut self.summary.workloads {
            if view.output.is_empty() {
                view.output = previous
                    .and_then(|previous| {
                        previous
                            .workloads
                            .iter()
                            .find(|(id, old)| id == project && old.id == view.id)
                    })
                    .map(|(_, old)| old.output.clone())
                    .unwrap_or_default();
            }
        }
        for patch in &self.logs {
            if let Some((_, view)) = self
                .summary
                .workloads
                .iter_mut()
                .find(|(project, view)| *project == patch.project && view.id == patch.workload)
            {
                let body = parts(&view.output).1;
                let kept = if patch.reset {
                    ""
                } else {
                    ensure!(
                        patch.trim <= body.len() && body.is_char_boundary(patch.trim),
                        "Log update needs a fresh snapshot."
                    );
                    &body[patch.trim..]
                };
                view.output = format!("{}{}{}", patch.header, kept, patch.text);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn patches_reproduce_unicode_append_and_bounded_tail_rotation() {
        for (old, new) in [
            ("first\n", "first\nsecond 测试\n"),
            (
                "[Earlier output omitted: 2 bytes]\nold\nkept\n",
                "[Earlier output omitted: 6 bytes]\nkept\nnew\n",
            ),
            ("unchanged", "unchanged"),
            ("old", "entirely different"),
            ("a", ""),
            ("", "new"),
        ] {
            let (head, body) = parts(new);
            let old_body = parts(old).1;
            let common = overlap(old_body, body);
            let rebuilt = format!(
                "{head}{}{}",
                &old_body[old_body.len() - common..],
                &body[common..]
            );
            assert_eq!(rebuilt, new);
        }
    }
}
