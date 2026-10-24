use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) const OWNER: &str = "io.jurabox.owner";
pub(super) const PROJECT: &str = "io.jurabox.project";
pub(super) const WORKLOAD: &str = "io.jurabox.workload";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Container {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(rename = "Created")]
    pub created: String,
    #[serde(rename = "Labels")]
    pub labels: BTreeMap<String, String>,
    #[serde(rename = "State")]
    pub state: State,
    #[serde(rename = "StopSignal", default)]
    pub stop_signal: String,
    #[serde(rename = "Restart")]
    pub restart: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct State {
    #[serde(rename = "Running")]
    pub running: bool,
    #[serde(rename = "Paused")]
    pub paused: bool,
    #[serde(rename = "Restarting")]
    pub restarting: bool,
    #[serde(rename = "ExitCode")]
    pub exit_code: i64,
    #[serde(rename = "Health")]
    pub health: Option<Health>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Health {
    #[serde(rename = "Status")]
    pub status: String,
}
impl Container {
    pub fn active(&self) -> bool {
        self.state.running || self.state.paused || self.state.restarting
    }
    pub fn service(&self) -> &str {
        self.labels
            .get("com.docker.compose.service")
            .map(String::as_str)
            .unwrap_or("")
    }
    pub fn normal_signal(&self) -> Result<&str> {
        let signal = if self.stop_signal.is_empty() {
            "SIGTERM"
        } else {
            &self.stop_signal
        };
        ensure!(
            !["KILL", "SIGKILL"].contains(&signal.to_ascii_uppercase().as_str())
                && signal.parse::<u32>().ok() != Some(9),
            "{} requests a forceful stop signal. Use explicit Force stop instead.",
            self.service()
        );
        ensure!(
            self.restart.is_empty() || self.restart == "no",
            "{} has a restart policy; normal shutdown is not safe.",
            self.service()
        );
        Ok(signal)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Plan {
    pub document: Value,
    pub order: Vec<String>,
    pub dependencies: BTreeMap<String, BTreeMap<String, String>>,
    pub finite: Vec<String>,
}
impl Plan {
    pub fn depends_on(&self, service: &str, target: &str) -> bool {
        self.dependencies.get(service).is_some_and(|deps| {
            deps.keys()
                .any(|dependency| dependency == target || self.depends_on(dependency, target))
        })
    }
}
pub(super) fn plan(
    mut document: Value,
    namespace: &str,
    owner: &str,
    project: &str,
    workload: &str,
) -> Result<Plan> {
    document["name"] = json!(namespace);
    let services = document["services"]
        .as_object_mut()
        .context("Compose file has no services")?;
    ensure!(
        !services.is_empty() && services.len() <= 32,
        "Use a Compose file with 1 to 32 services."
    );
    let mut dependencies = BTreeMap::new();
    let mut finite = Vec::new();
    for (name, service) in services.iter_mut() {
        ensure!(
            service.get("container_name").is_none(),
            "{name}: remove container_name so Jurabox can isolate its containers."
        );
        ensure!(
            service.get("profiles").is_none(),
            "{name}: Compose profiles are not supported yet; use a file containing the desired services."
        );
        ensure!(
            service.get("post_start").is_none() && service.get("pre_stop").is_none(),
            "{name}: Compose lifecycle hooks are not supported yet."
        );
        ensure!(
            service.get("restart").is_none_or(|value| value == "no")
                && service.pointer("/deploy/restart_policy").is_none(),
            "{name}: disable restart policies so stopping cannot restart containers automatically."
        );
        ensure!(
            service.get("scale").is_none_or(|value| value == 1)
                && service
                    .pointer("/deploy/replicas")
                    .is_none_or(|value| value == 1),
            "{name}: only one container per Compose service is supported initially."
        );
        ensure!(
            service.get("image").and_then(Value::as_str).is_some(),
            "{name}: build the image first and set its image name."
        );
        let mut required = BTreeMap::new();
        if let Some(deps) = service.get("depends_on").and_then(Value::as_object) {
            for (dependency, settings) in deps {
                ensure!(
                    settings.get("restart").is_none_or(|value| value == false)
                        && settings.get("required").is_none_or(|value| value == true),
                    "{name}: optional or restarting dependencies are not supported."
                );
                let condition = settings["condition"].as_str().unwrap_or("service_started");
                ensure!(
                    [
                        "service_started",
                        "service_healthy",
                        "service_completed_successfully"
                    ]
                    .contains(&condition),
                    "Unsupported Compose dependency condition"
                );
                if condition == "service_completed_successfully" && !finite.contains(dependency) {
                    finite.push(dependency.clone());
                }
                required.insert(dependency.clone(), condition.to_owned());
            }
        }
        for key in ["network_mode", "ipc", "pid"] {
            if let Some(value) = service
                .get(key)
                .and_then(Value::as_str)
                .and_then(|value| value.strip_prefix("service:"))
            {
                required
                    .entry(value.to_owned())
                    .or_insert("service_started".into());
            }
        }
        dependencies.insert(name.clone(), required);
        let mut labels = service
            .get("labels")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        ensure!(
            !labels.keys().any(|key| key.starts_with("io.jurabox.")),
            "{name}: io.jurabox labels are reserved."
        );
        labels.insert(OWNER.into(), json!(owner));
        labels.insert(PROJECT.into(), json!(project));
        labels.insert(WORKLOAD.into(), json!(workload));
        service["labels"] = labels.into();
    }
    let mut order: Vec<String> = Vec::new();
    while order.len() < services.len() {
        let wave: Vec<_> = services
            .keys()
            .filter(|name| {
                !order.contains(*name)
                    && dependencies[name.as_str()]
                        .keys()
                        .all(|dependency| order.contains(dependency))
            })
            .cloned()
            .collect();
        ensure!(
            !wave.is_empty(),
            "Compose dependencies contain a cycle or unknown service."
        );
        order.extend(wave);
    }
    // Compose's config serializer already escapes literal dollars for reuse as
    // a Compose input. Preserve that representation; escaping again changes commands.
    Ok(Plan {
        document,
        order,
        dependencies,
        finite,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compose_plan_orders_dependencies_injects_ownership_and_preserves_dollars() {
        let result = plan(json!({"services":{"web":{"image":"test","environment":{"LITERAL":"$$HOME"},"depends_on":{"db":{"condition":"service_healthy"}}},"db":{"image":"test"}}}), "private", "token", "project", "workload").unwrap();
        assert_eq!(result.order, ["db", "web"]);
        assert_eq!(
            result.document["services"]["web"]["environment"]["LITERAL"],
            "$$HOME"
        );
        assert_eq!(result.document["services"]["db"]["labels"][OWNER], "token");
    }
    #[test]
    fn compose_rejects_aliases_restarts_hooks_and_cycles_before_creation() {
        for patch in [
            json!({"container_name":"existing"}),
            json!({"restart":"always"}),
            json!({"post_start":[]}),
            json!({"profiles":["dev"]}),
            json!({"scale":2}),
            json!({"labels":{OWNER:"spoof"}}),
            json!({"depends_on":{"db":{"condition":"service_started"}}}),
        ] {
            let mut service = json!({"image":"test"});
            service
                .as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            assert!(
                plan(
                    json!({"services":{"db":service}}),
                    "private",
                    "token",
                    "project",
                    "workload"
                )
                .is_err()
            );
        }
    }
}
