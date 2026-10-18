use super::*;
use std::collections::HashSet;

impl Services {
    pub(super) fn ready_ids(&self) -> HashSet<Uuid> {
        let mut ready = HashSet::new();
        loop {
            let before = ready.len();
            for entry in &self.entries {
                if entry.own_ready()
                    && entry
                        .config
                        .dependencies
                        .iter()
                        .all(|id| ready.contains(id))
                {
                    ready.insert(entry.config.id);
                }
            }
            if before == ready.len() {
                return ready;
            }
        }
    }

    pub(super) fn missing_dependencies(&self, index: usize, ready: &HashSet<Uuid>) -> Vec<String> {
        self.entries[index]
            .config
            .dependencies
            .iter()
            .filter(|id| !ready.contains(id))
            .map(|id| {
                self.entries
                    .iter()
                    .find(|entry| entry.config.id == *id)
                    .map(|entry| format!("{} ({})", entry.config.name, entry.view().status))
                    .unwrap_or_else(|| "Unknown dependency".into())
            })
            .collect()
    }

    pub(super) fn schedule(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        loop {
            let mut started = false;
            let ready = self.ready_ids();
            for index in 0..self.entries.len() {
                if self.entries[index].status != Status::Waiting {
                    continue;
                }
                if !self.missing_dependencies(index, &ready).is_empty() {
                    self.entries[index].preflight_clear = false;
                    continue;
                }
                let entry = &mut self.entries[index];
                if entry.config.readiness != Readiness::ProcessRunning && !entry.preflight_clear {
                    entry.spawn_probe(true);
                } else {
                    if let Err(error) = entry.start() {
                        errors.push(format!("{error:#}"));
                    }
                    started = true;
                }
            }
            if !started {
                break;
            }
        }
        errors
    }

    fn depends_on(&self, index: usize, target: Uuid) -> bool {
        self.entries[index].config.dependencies.iter().any(|id| {
            *id == target
                || self
                    .entries
                    .iter()
                    .position(|entry| entry.config.id == *id)
                    .is_some_and(|index| self.depends_on(index, target))
        })
    }

    pub(super) fn live_dependents(&self, id: Uuid) -> Vec<String> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(index, entry)| entry.live() && self.depends_on(*index, id))
            .map(|(_, entry)| entry.config.name.clone())
            .collect()
    }

    pub(super) fn shutdown_order(&self) -> Vec<usize> {
        let mut indices = Vec::new();
        let mut added = HashSet::new();
        while indices.len() != self.entries.len() {
            let before = indices.len();
            for (index, entry) in self.entries.iter().enumerate() {
                if !added.contains(&entry.config.id)
                    && entry
                        .config
                        .dependencies
                        .iter()
                        .all(|id| added.contains(id))
                {
                    added.insert(entry.config.id);
                    indices.push(index);
                }
            }
            if before == indices.len() {
                break;
            } // Validation rejects cycles/missing IDs.
        }
        indices.reverse();
        indices
    }
}
