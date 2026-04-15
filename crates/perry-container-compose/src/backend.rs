use crate::error::{ComposeError, Result};
use crate::types::{
    ComposeNetwork, ComposeServiceBuild, ComposeVolume, ContainerHandle, ContainerInfo,
    ContainerLogs, ContainerSpec, ImageInfo,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendProbeResult {
    pub name: String,
    pub available: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct SecurityProfile {
    pub read_only_root: bool,
    pub seccomp: Option<String>,
}

#[async_trait]
pub trait ContainerBackend: Send + Sync {
    fn backend_name(&self) -> &str;
    async fn check_available(&self) -> Result<()>;
    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerHandle>;
    async fn create(&self, spec: &ContainerSpec) -> Result<ContainerHandle>;
    async fn start(&self, id: &str) -> Result<()>;
    async fn stop(&self, id: &str, timeout: Option<u32>) -> Result<()>;
    async fn remove(&self, id: &str, force: bool) -> Result<()>;
    async fn list(&self, all: bool) -> Result<Vec<ContainerInfo>>;
    async fn inspect(&self, id: &str) -> Result<ContainerInfo>;
    async fn logs(&self, id: &str, tail: Option<u32>) -> Result<ContainerLogs>;
    async fn exec(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Result<ContainerLogs>;
    async fn pull_image(&self, reference: &str) -> Result<()>;
    async fn list_images(&self) -> Result<Vec<ImageInfo>>;
    async fn remove_image(&self, reference: &str, force: bool) -> Result<()>;
    async fn create_network(&self, name: &str, config: &ComposeNetwork) -> Result<()>;
    async fn remove_network(&self, name: &str) -> Result<()>;
    async fn create_volume(&self, name: &str, config: &ComposeVolume) -> Result<()>;
    async fn remove_volume(&self, name: &str) -> Result<()>;
    async fn inspect_network(&self, name: &str) -> Result<()>;
    async fn inspect_volume(&self, name: &str) -> Result<()>;
    async fn inspect_image(&self, reference: &str) -> Result<ImageInfo>;
    async fn build(&self, spec: &ComposeServiceBuild, image_name: &str) -> Result<()>;
    async fn run_with_security(
        &self,
        spec: &ContainerSpec,
        profile: &SecurityProfile,
    ) -> Result<ContainerHandle>;
    /// Wait for a container to exit and return its exit code.
    async fn wait(&self, id: &str) -> Result<i32>;
}

pub trait CliProtocol: Send + Sync {
    fn subcommand_prefix(&self) -> Option<&str> {
        None
    }

    fn run_args(&self, spec: &ContainerSpec) -> Vec<String>;
    fn create_args(&self, spec: &ContainerSpec) -> Vec<String>;
    fn start_args(&self, id: &str) -> Vec<String>;
    fn stop_args(&self, id: &str, timeout: Option<u32>) -> Vec<String>;
    fn remove_args(&self, id: &str, force: bool) -> Vec<String>;
    fn list_args(&self, all: bool) -> Vec<String>;
    fn inspect_args(&self, id: &str) -> Vec<String>;
    fn logs_args(&self, id: &str, tail: Option<u32>) -> Vec<String>;
    fn exec_args(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Vec<String>;
    fn pull_image_args(&self, reference: &str) -> Vec<String>;
    fn list_images_args(&self) -> Vec<String>;
    fn remove_image_args(&self, reference: &str, force: bool) -> Vec<String>;
    fn create_network_args(&self, name: &str, config: &ComposeNetwork) -> Vec<String>;
    fn remove_network_args(&self, name: &str) -> Vec<String>;
    fn create_volume_args(&self, name: &str, config: &ComposeVolume) -> Vec<String>;
    fn remove_volume_args(&self, name: &str) -> Vec<String>;
    fn inspect_network_args(&self, name: &str) -> Vec<String>;
    fn inspect_volume_args(&self, name: &str) -> Vec<String>;
    fn inspect_image_args(&self, reference: &str) -> Vec<String>;
    fn build_args(&self, spec: &ComposeServiceBuild, image_name: &str) -> Vec<String>;
    fn security_args(&self, profile: &SecurityProfile) -> Vec<String>;

    fn parse_list_output(&self, stdout: &str) -> Result<Vec<ContainerInfo>>;
    fn parse_inspect_output(&self, stdout: &str) -> Result<ContainerInfo>;
    fn parse_list_images_output(&self, stdout: &str) -> Result<Vec<ImageInfo>>;
    fn parse_container_id(&self, stdout: &str) -> Result<String>;
}

#[derive(Debug, Deserialize)]
struct DockerListEntry {
    #[serde(rename = "ID", alias = "Id", default)]
    id: String,
    #[serde(rename = "Names", default)]
    names: Vec<String>,
    #[serde(rename = "Image", default)]
    image: String,
    #[serde(rename = "Status", alias = "State", default)]
    status: String,
    #[serde(rename = "Ports", default)]
    ports: Vec<String>,
    #[serde(rename = "Labels", default)]
    labels: serde_json::Value,
    #[serde(rename = "Created", alias = "CreatedAt", default)]
    created: String,
}

#[derive(Debug, Deserialize)]
struct DockerInspectOutput {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Config")]
    config: DockerInspectConfig,
    #[serde(rename = "State")]
    state: DockerInspectState,
    #[serde(rename = "Created")]
    created: String,
    #[serde(rename = "NetworkSettings", default)]
    network_settings: Option<DockerInspectNetworkSettings>,
}

#[derive(Debug, Deserialize)]
struct DockerInspectConfig {
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "Labels", default)]
    labels: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct DockerInspectState {
    #[serde(rename = "Status")]
    status: String,
}

#[derive(Debug, Deserialize)]
struct DockerInspectNetworkSettings {
    #[serde(rename = "IPAddress", default)]
    ip_address: String,
    #[serde(rename = "Networks", default)]
    networks: HashMap<String, DockerInspectNetwork>,
}

#[derive(Debug, Deserialize)]
struct DockerInspectNetwork {
    #[serde(rename = "IPAddress", default)]
    ip_address: String,
}

#[derive(Debug, Deserialize)]
struct DockerImageEntry {
    #[serde(rename = "ID", alias = "Id", default)]
    id: String,
    #[serde(rename = "Repositories", alias = "Repository", default)]
    repository: String,
    #[serde(rename = "Tag", default)]
    tag: String,
    #[serde(rename = "Size", default)]
    size: u64,
    #[serde(rename = "Created", alias = "CreatedAt", default)]
    created: String,
}

pub struct DockerProtocol;

impl CliProtocol for DockerProtocol {
    fn run_args(&self, spec: &ContainerSpec) -> Vec<String> {
        let mut args = vec!["run".into(), "--detach".into()];
        if let Some(name) = &spec.name {
            args.extend(["--name".into(), name.clone()]);
        }
        for port in spec.ports.as_ref().iter().flat_map(|v| v.iter()) {
            args.extend(["-p".into(), port.clone()]);
        }
        for vol in spec.volumes.as_ref().iter().flat_map(|v| v.iter()) {
            args.extend(["-v".into(), vol.clone()]);
        }
        for (k, v) in spec.env.as_ref().iter().flat_map(|m| m.iter()) {
            args.extend(["-e".into(), format!("{k}={v}")]);
        }
        for (k, v) in spec.labels.as_ref().iter().flat_map(|m| m.iter()) {
            args.extend(["--label".into(), format!("{k}={v}")]);
        }
        if let Some(net) = &spec.network {
            args.extend(["--network".into(), net.clone()]);
        }
        if spec.rm.unwrap_or(false) {
            args.push("--rm".into());
        }
        if spec.read_only.unwrap_or(false) {
            args.push("--read-only".into());
        }
        if spec.privileged.unwrap_or(false) {
            args.push("--privileged".into());
        }
        if let Some(user) = &spec.user {
            args.extend(["--user".into(), user.clone()]);
        }
        if let Some(wd) = &spec.workdir {
            args.extend(["--workdir".into(), wd.clone()]);
        }
        if let Some(caps) = &spec.cap_add {
            for cap in caps {
                args.extend(["--cap-add".into(), cap.clone()]);
            }
        }
        if let Some(caps) = &spec.cap_drop {
            for cap in caps {
                args.extend(["--cap-drop".into(), cap.clone()]);
            }
        }
        if let Some(ep) = &spec.entrypoint {
            args.push("--entrypoint".into());
            args.push(ep.join(" "));
        }
        args.push(spec.image.clone());
        for c in spec.cmd.as_ref().iter().flat_map(|v| v.iter()) {
            args.push(c.clone());
        }
        args
    }

    fn create_args(&self, spec: &ContainerSpec) -> Vec<String> {
        let mut args = vec!["create".into()];
        if let Some(name) = &spec.name {
            args.extend(["--name".into(), name.clone()]);
        }
        for port in spec.ports.as_ref().iter().flat_map(|v| v.iter()) {
            args.extend(["-p".into(), port.clone()]);
        }
        for vol in spec.volumes.as_ref().iter().flat_map(|v| v.iter()) {
            args.extend(["-v".into(), vol.clone()]);
        }
        for (k, v) in spec.env.as_ref().iter().flat_map(|m| m.iter()) {
            args.extend(["-e".into(), format!("{k}={v}")]);
        }
        for (k, v) in spec.labels.as_ref().iter().flat_map(|m| m.iter()) {
            args.extend(["--label".into(), format!("{k}={v}")]);
        }
        if let Some(net) = &spec.network {
            args.extend(["--network".into(), net.clone()]);
        }
        if spec.read_only.unwrap_or(false) {
            args.push("--read-only".into());
        }
        if spec.privileged.unwrap_or(false) {
            args.push("--privileged".into());
        }
        if let Some(user) = &spec.user {
            args.extend(["--user".into(), user.clone()]);
        }
        if let Some(wd) = &spec.workdir {
            args.extend(["--workdir".into(), wd.clone()]);
        }
        if let Some(caps) = &spec.cap_add {
            for cap in caps {
                args.extend(["--cap-add".into(), cap.clone()]);
            }
        }
        if let Some(caps) = &spec.cap_drop {
            for cap in caps {
                args.extend(["--cap-drop".into(), cap.clone()]);
            }
        }
        if let Some(ep) = &spec.entrypoint {
            args.push("--entrypoint".into());
            args.push(ep.join(" "));
        }
        args.push(spec.image.clone());
        for c in spec.cmd.as_ref().iter().flat_map(|v| v.iter()) {
            args.push(c.clone());
        }
        args
    }

    fn start_args(&self, id: &str) -> Vec<String> {
        vec!["start".into(), id.into()]
    }

    fn stop_args(&self, id: &str, timeout: Option<u32>) -> Vec<String> {
        let mut args = vec!["stop".into()];
        if let Some(t) = timeout {
            args.extend(["--time".into(), t.to_string()]);
        }
        args.push(id.into());
        args
    }

    fn remove_args(&self, id: &str, force: bool) -> Vec<String> {
        let mut args = vec!["rm".into()];
        if force {
            args.push("-f".into());
        }
        args.push(id.into());
        args
    }

    fn list_args(&self, all: bool) -> Vec<String> {
        let mut args = vec!["ps".into(), "--format".into(), "json".into()];
        if all {
            args.push("--all".into());
        }
        args
    }

    fn inspect_args(&self, id: &str) -> Vec<String> {
        vec![
            "inspect".into(),
            "--format".into(),
            "json".into(),
            id.into(),
        ]
    }

    fn logs_args(&self, id: &str, tail: Option<u32>) -> Vec<String> {
        let mut args = vec!["logs".into()];
        if let Some(t) = tail {
            args.extend(["--tail".into(), t.to_string()]);
        }
        args.push(id.into());
        args
    }

    fn exec_args(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Vec<String> {
        let mut args = vec!["exec".into()];
        if let Some(w) = workdir {
            args.extend(["--workdir".into(), w.into()]);
        }
        if let Some(e) = env {
            for (k, v) in e {
                args.extend(["-e".into(), format!("{k}={v}")]);
            }
        }
        args.push(id.into());
        args.extend(cmd.iter().cloned());
        args
    }

    fn pull_image_args(&self, reference: &str) -> Vec<String> {
        vec!["pull".into(), reference.into()]
    }

    fn list_images_args(&self) -> Vec<String> {
        vec!["images".into(), "--format".into(), "json".into()]
    }

    fn remove_image_args(&self, reference: &str, force: bool) -> Vec<String> {
        let mut args = vec!["rmi".into()];
        if force {
            args.push("-f".into());
        }
        args.push(reference.into());
        args
    }

    fn create_network_args(&self, name: &str, config: &ComposeNetwork) -> Vec<String> {
        let mut args = vec!["network".into(), "create".into()];
        if let Some(d) = &config.driver {
            args.extend(["--driver".into(), d.clone()]);
        }
        if let Some(lbls) = &config.labels {
            for (k, v) in lbls.to_map() {
                args.extend(["--label".into(), format!("{k}={v}")]);
            }
        }
        args.push(name.into());
        args
    }

    fn remove_network_args(&self, name: &str) -> Vec<String> {
        vec!["network".into(), "rm".into(), name.into()]
    }

    fn create_volume_args(&self, name: &str, config: &ComposeVolume) -> Vec<String> {
        let mut args = vec!["volume".into(), "create".into()];
        if let Some(d) = &config.driver {
            args.extend(["--driver".into(), d.clone()]);
        }
        if let Some(lbls) = &config.labels {
            for (k, v) in lbls.to_map() {
                args.extend(["--label".into(), format!("{k}={v}")]);
            }
        }
        args.push(name.into());
        args
    }

    fn remove_volume_args(&self, name: &str) -> Vec<String> {
        vec!["volume".into(), "rm".into(), name.into()]
    }

    fn inspect_network_args(&self, name: &str) -> Vec<String> {
        vec!["network".into(), "inspect".into(), name.into()]
    }

    fn inspect_volume_args(&self, name: &str) -> Vec<String> {
        vec!["volume".into(), "inspect".into(), name.into()]
    }

    fn inspect_image_args(&self, reference: &str) -> Vec<String> {
        vec![
            "inspect".into(),
            "--format".into(),
            "json".into(),
            reference.into(),
        ]
    }

    fn build_args(&self, spec: &ComposeServiceBuild, image_name: &str) -> Vec<String> {
        let mut args = vec!["build".into(), "-t".into(), image_name.to_string()];
        if let Some(ref f) = spec.containerfile {
            args.extend(["-f".into(), f.clone()]);
        }
        args.push(spec.context.as_deref().unwrap_or(".").to_string());
        args
    }

    fn security_args(&self, profile: &SecurityProfile) -> Vec<String> {
        let mut args = Vec::new();
        if profile.read_only_root {
            args.push("--read-only".into());
        }
        if let Some(seccomp) = &profile.seccomp {
            args.extend(["--security-opt".into(), format!("seccomp={}", seccomp)]);
        }
        args
    }

    fn parse_list_output(&self, stdout: &str) -> Result<Vec<ContainerInfo>> {
        let entries: Vec<DockerListEntry> = stdout
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        Ok(entries
            .into_iter()
            .map(|e| {
                let mut labels = HashMap::new();
                if let Some(map) = e.labels.as_object() {
                    for (k, v) in map {
                        labels.insert(k.clone(), v.as_str().unwrap_or("").to_string());
                    }
                } else if let Some(s) = e.labels.as_str() {
                    // Handle comma-separated labels if necessary
                    for pair in s.split(',') {
                        let mut parts = pair.splitn(2, '=');
                        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                            labels.insert(k.to_string(), v.to_string());
                        }
                    }
                }

                ContainerInfo {
                    id: e.id,
                    name: e.names.first().cloned().unwrap_or_default(),
                    image: e.image,
                    status: e.status,
                    ports: e.ports,
                    labels,
                    created: e.created,
                    ip_address: String::new(),
                }
            })
            .collect())
    }

    fn parse_inspect_output(&self, stdout: &str) -> Result<ContainerInfo> {
        let entries: Vec<DockerInspectOutput> = serde_json::from_str(stdout)?;
        let e = entries
            .into_iter()
            .next()
            .ok_or_else(|| ComposeError::NotFound("Inspect output empty".into()))?;

        let mut ip_address = String::new();
        if let Some(settings) = &e.network_settings {
            if !settings.ip_address.is_empty() {
                ip_address = settings.ip_address.clone();
            } else {
                // Try to get from first network
                if let Some(net) = settings.networks.values().next() {
                    ip_address = net.ip_address.clone();
                }
            }
        }

        Ok(ContainerInfo {
            id: e.id,
            name: e.name,
            image: e.config.image,
            status: e.state.status,
            ports: vec![],
            labels: e.config.labels,
            created: e.created,
            ip_address,
        })
    }

    fn parse_list_images_output(&self, stdout: &str) -> Result<Vec<ImageInfo>> {
        let entries: Vec<DockerImageEntry> = stdout
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        Ok(entries
            .into_iter()
            .map(|e| ImageInfo {
                id: e.id,
                repository: e.repository,
                tag: e.tag,
                size: e.size,
                created: e.created,
            })
            .collect())
    }

    fn parse_container_id(&self, stdout: &str) -> Result<String> {
        Ok(stdout.trim().to_string())
    }
}

pub struct AppleContainerProtocol;

impl CliProtocol for AppleContainerProtocol {
    fn run_args(&self, spec: &ContainerSpec) -> Vec<String> {
        let mut args = vec!["run".into()];
        if spec.rm.unwrap_or(false) {
            args.push("--rm".into());
        }
        if let Some(name) = &spec.name {
            args.extend(["--name".into(), name.clone()]);
        }
        if let Some(network) = &spec.network {
            args.extend(["--network".into(), network.clone()]);
        }
        for port in spec.ports.as_ref().iter().flat_map(|v| v.iter()) {
            args.extend(["-p".into(), port.clone()]);
        }
        for vol in spec.volumes.as_ref().iter().flat_map(|v| v.iter()) {
            args.extend(["-v".into(), vol.clone()]);
        }
        for (k, v) in spec.env.as_ref().iter().flat_map(|m| m.iter()) {
            args.extend(["-e".into(), format!("{k}={v}")]);
        }
        if spec.read_only.unwrap_or(false) {
            args.push("--read-only".into());
        }
        if spec.privileged.unwrap_or(false) {
            args.push("--privileged".into());
        }
        if let Some(user) = &spec.user {
            args.extend(["--user".into(), user.clone()]);
        }
        if let Some(wd) = &spec.workdir {
            args.extend(["--workdir".into(), wd.clone()]);
        }
        if let Some(caps) = &spec.cap_add {
            for cap in caps {
                args.extend(["--cap-add".into(), cap.clone()]);
            }
        }
        if let Some(caps) = &spec.cap_drop {
            for cap in caps {
                args.extend(["--cap-drop".into(), cap.clone()]);
            }
        }
        args.push(spec.image.clone());
        for c in spec.cmd.as_ref().iter().flat_map(|v| v.iter()) {
            args.push(c.clone());
        }
        args
    }

    fn create_args(&self, spec: &ContainerSpec) -> Vec<String> {
        DockerProtocol.create_args(spec)
    }
    fn start_args(&self, id: &str) -> Vec<String> {
        DockerProtocol.start_args(id)
    }
    fn stop_args(&self, id: &str, timeout: Option<u32>) -> Vec<String> {
        DockerProtocol.stop_args(id, timeout)
    }
    fn remove_args(&self, id: &str, force: bool) -> Vec<String> {
        DockerProtocol.remove_args(id, force)
    }
    fn list_args(&self, all: bool) -> Vec<String> {
        DockerProtocol.list_args(all)
    }
    fn inspect_args(&self, id: &str) -> Vec<String> {
        DockerProtocol.inspect_args(id)
    }
    fn logs_args(&self, id: &str, tail: Option<u32>) -> Vec<String> {
        DockerProtocol.logs_args(id, tail)
    }
    fn exec_args(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Vec<String> {
        DockerProtocol.exec_args(id, cmd, env, workdir)
    }
    fn pull_image_args(&self, reference: &str) -> Vec<String> {
        DockerProtocol.pull_image_args(reference)
    }
    fn list_images_args(&self) -> Vec<String> {
        DockerProtocol.list_images_args()
    }
    fn remove_image_args(&self, reference: &str, force: bool) -> Vec<String> {
        DockerProtocol.remove_image_args(reference, force)
    }
    fn create_network_args(&self, name: &str, config: &ComposeNetwork) -> Vec<String> {
        DockerProtocol.create_network_args(name, config)
    }
    fn remove_network_args(&self, name: &str) -> Vec<String> {
        DockerProtocol.remove_network_args(name)
    }
    fn create_volume_args(&self, name: &str, config: &ComposeVolume) -> Vec<String> {
        DockerProtocol.create_volume_args(name, config)
    }
    fn remove_volume_args(&self, name: &str) -> Vec<String> {
        DockerProtocol.remove_volume_args(name)
    }
    fn inspect_network_args(&self, name: &str) -> Vec<String> {
        DockerProtocol.inspect_network_args(name)
    }
    fn inspect_volume_args(&self, name: &str) -> Vec<String> {
        DockerProtocol.inspect_volume_args(name)
    }
    fn inspect_image_args(&self, reference: &str) -> Vec<String> {
        DockerProtocol.inspect_image_args(reference)
    }
    fn build_args(&self, spec: &ComposeServiceBuild, image_name: &str) -> Vec<String> {
        DockerProtocol.build_args(spec, image_name)
    }
    fn security_args(&self, profile: &SecurityProfile) -> Vec<String> {
        DockerProtocol.security_args(profile)
    }
    fn parse_list_output(&self, stdout: &str) -> Result<Vec<ContainerInfo>> {
        DockerProtocol.parse_list_output(stdout)
    }
    fn parse_inspect_output(&self, stdout: &str) -> Result<ContainerInfo> {
        DockerProtocol.parse_inspect_output(stdout)
    }
    fn parse_list_images_output(&self, stdout: &str) -> Result<Vec<ImageInfo>> {
        DockerProtocol.parse_list_images_output(stdout)
    }
    fn parse_container_id(&self, stdout: &str) -> Result<String> {
        DockerProtocol.parse_container_id(stdout)
    }
}

pub struct LimaProtocol {
    pub instance: String,
}

impl CliProtocol for LimaProtocol {
    fn run_args(&self, spec: &ContainerSpec) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.run_args(spec));
        args
    }
    fn create_args(&self, spec: &ContainerSpec) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.create_args(spec));
        args
    }
    fn start_args(&self, id: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.start_args(id));
        args
    }
    fn stop_args(&self, id: &str, timeout: Option<u32>) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.stop_args(id, timeout));
        args
    }
    fn remove_args(&self, id: &str, force: bool) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_args(id, force));
        args
    }
    fn list_args(&self, all: bool) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.list_args(all));
        args
    }
    fn inspect_args(&self, id: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_args(id));
        args
    }
    fn logs_args(&self, id: &str, tail: Option<u32>) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.logs_args(id, tail));
        args
    }
    fn exec_args(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.exec_args(id, cmd, env, workdir));
        args
    }
    fn pull_image_args(&self, reference: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.pull_image_args(reference));
        args
    }
    fn list_images_args(&self) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.list_images_args());
        args
    }
    fn remove_image_args(&self, reference: &str, force: bool) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_image_args(reference, force));
        args
    }
    fn create_network_args(&self, name: &str, config: &ComposeNetwork) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.create_network_args(name, config));
        args
    }
    fn remove_network_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_network_args(name));
        args
    }
    fn create_volume_args(&self, name: &str, config: &ComposeVolume) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.create_volume_args(name, config));
        args
    }
    fn remove_volume_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.remove_volume_args(name));
        args
    }
    fn inspect_network_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_network_args(name));
        args
    }
    fn inspect_volume_args(&self, name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_volume_args(name));
        args
    }
    fn inspect_image_args(&self, reference: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.inspect_image_args(reference));
        args
    }
    fn build_args(&self, spec: &ComposeServiceBuild, image_name: &str) -> Vec<String> {
        let mut args = vec!["shell".into(), self.instance.clone(), "nerdctl".into()];
        args.extend(DockerProtocol.build_args(spec, image_name));
        args
    }
    fn security_args(&self, profile: &SecurityProfile) -> Vec<String> {
        // Return only the nerdctl flags, the caller (run_with_security) will insert them
        // into the already prefixed run_args.
        DockerProtocol.security_args(profile)
    }
    fn parse_list_output(&self, stdout: &str) -> Result<Vec<ContainerInfo>> {
        DockerProtocol.parse_list_output(stdout)
    }
    fn parse_inspect_output(&self, stdout: &str) -> Result<ContainerInfo> {
        DockerProtocol.parse_inspect_output(stdout)
    }
    fn parse_list_images_output(&self, stdout: &str) -> Result<Vec<ImageInfo>> {
        DockerProtocol.parse_list_images_output(stdout)
    }
    fn parse_container_id(&self, stdout: &str) -> Result<String> {
        DockerProtocol.parse_container_id(stdout)
    }
}

pub struct CliBackend {
    pub bin: PathBuf,
    pub protocol: Box<dyn CliProtocol>,
}

impl CliBackend {
    pub fn new(bin: PathBuf, protocol: Box<dyn CliProtocol>) -> Self {
        Self { bin, protocol }
    }

    async fn exec_raw(&self, args: &[String]) -> Result<(String, String)> {
        let output = Command::new(&self.bin)
            .args(args)
            .output()
            .await
            .map_err(ComposeError::IoError)?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok((stdout, stderr))
        } else {
            Err(ComposeError::BackendError {
                code: output.status.code().unwrap_or(-1),
                message: stderr,
            })
        }
    }
}

#[async_trait]
impl ContainerBackend for CliBackend {
    fn backend_name(&self) -> &str {
        self.bin
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
    }

    async fn check_available(&self) -> Result<()> {
        Command::new(&self.bin)
            .arg("--version")
            .output()
            .await
            .map_err(ComposeError::IoError)
            .map(|_| ())
    }

    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerHandle> {
        let args = self.protocol.run_args(spec);
        let (stdout, _) = self.exec_raw(&args).await?;
        let id = self.protocol.parse_container_id(&stdout)?;
        Ok(ContainerHandle {
            id,
            name: spec.name.clone(),
        })
    }

    async fn create(&self, spec: &ContainerSpec) -> Result<ContainerHandle> {
        let args = self.protocol.create_args(spec);
        let (stdout, _) = self.exec_raw(&args).await?;
        let id = self.protocol.parse_container_id(&stdout)?;
        Ok(ContainerHandle {
            id,
            name: spec.name.clone(),
        })
    }

    async fn start(&self, id: &str) -> Result<()> {
        let args = self.protocol.start_args(id);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn stop(&self, id: &str, timeout: Option<u32>) -> Result<()> {
        let args = self.protocol.stop_args(id, timeout);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn remove(&self, id: &str, force: bool) -> Result<()> {
        let args = self.protocol.remove_args(id, force);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn list(&self, all: bool) -> Result<Vec<ContainerInfo>> {
        let args = self.protocol.list_args(all);
        let (stdout, _) = self.exec_raw(&args).await?;
        self.protocol.parse_list_output(&stdout)
    }

    async fn inspect(&self, id: &str) -> Result<ContainerInfo> {
        let args = self.protocol.inspect_args(id);
        let (stdout, _) = self.exec_raw(&args).await?;
        self.protocol.parse_inspect_output(&stdout)
    }

    async fn logs(&self, id: &str, tail: Option<u32>) -> Result<ContainerLogs> {
        let args = self.protocol.logs_args(id, tail);
        let (stdout, stderr) = self.exec_raw(&args).await?;
        Ok(ContainerLogs { stdout, stderr })
    }

    async fn exec(
        &self,
        id: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Result<ContainerLogs> {
        let args = self.protocol.exec_args(id, cmd, env, workdir);
        let (stdout, stderr) = self.exec_raw(&args).await?;
        Ok(ContainerLogs { stdout, stderr })
    }

    async fn pull_image(&self, reference: &str) -> Result<()> {
        let args = self.protocol.pull_image_args(reference);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn list_images(&self) -> Result<Vec<ImageInfo>> {
        let args = self.protocol.list_images_args();
        let (stdout, _) = self.exec_raw(&args).await?;
        self.protocol.parse_list_images_output(&stdout)
    }

    async fn remove_image(&self, reference: &str, force: bool) -> Result<()> {
        let args = self.protocol.remove_image_args(reference, force);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn create_network(&self, name: &str, config: &ComposeNetwork) -> Result<()> {
        let args = self.protocol.create_network_args(name, config);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn remove_network(&self, name: &str) -> Result<()> {
        let args = self.protocol.remove_network_args(name);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn create_volume(&self, name: &str, config: &ComposeVolume) -> Result<()> {
        let args = self.protocol.create_volume_args(name, config);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn remove_volume(&self, name: &str) -> Result<()> {
        let args = self.protocol.remove_volume_args(name);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn inspect_network(&self, name: &str) -> Result<()> {
        let args = self.protocol.inspect_network_args(name);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn inspect_volume(&self, name: &str) -> Result<()> {
        let args = self.protocol.inspect_volume_args(name);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn inspect_image(&self, reference: &str) -> Result<ImageInfo> {
        let args = self.protocol.inspect_image_args(reference);
        let (stdout, _) = self.exec_raw(&args).await?;
        let images = self.protocol.parse_list_images_output(&stdout)?;
        images
            .into_iter()
            .next()
            .ok_or_else(|| ComposeError::NotFound(reference.to_string()))
    }

    async fn build(&self, spec: &ComposeServiceBuild, image_name: &str) -> Result<()> {
        let args = self.protocol.build_args(spec, image_name);
        self.exec_raw(&args).await.map(|_| ())
    }

    async fn run_with_security(
        &self,
        spec: &ContainerSpec,
        profile: &SecurityProfile,
    ) -> Result<ContainerHandle> {
        let mut args = self.protocol.run_args(spec);
        // Find the image name to insert security args before it
        if let Some(pos) = args.iter().position(|a| a == &spec.image) {
            let sec_args = self.protocol.security_args(profile);
            // If it's lima, we need to be careful with where we insert.
            // But let's assume we can just insert before the image.
            for (i, arg) in sec_args.into_iter().enumerate() {
                args.insert(pos + i, arg);
            }
        }

        let (stdout, _) = self.exec_raw(&args).await?;
        let id = self.protocol.parse_container_id(&stdout)?;
        Ok(ContainerHandle {
            id,
            name: spec.name.clone(),
        })
    }

    async fn wait(&self, id: &str) -> Result<i32> {
        // `docker/podman wait <id>` blocks until the container exits and prints the exit code.
        let output = Command::new(&self.bin)
            .args(["wait", id])
            .output()
            .await
            .map_err(ComposeError::IoError)?;
        let code_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(code_str.parse::<i32>().unwrap_or(-1))
    }
}

pub async fn detect_backend() -> Result<Box<dyn ContainerBackend>> {
    if let Ok(name) = std::env::var("PERRY_CONTAINER_BACKEND") {
        return probe_candidate(&name)
            .await
            .map_err(|reason| ComposeError::NoBackendFound {
                probed: vec![BackendProbeResult {
                    name: name.clone(),
                    available: false,
                    reason,
                }],
            });
    }

    let candidates = platform_candidates();
    let mut results = Vec::new();

    for candidate in candidates {
        match tokio::time::timeout(Duration::from_secs(2), probe_candidate(candidate)).await {
            Ok(Ok(backend)) => return Ok(backend),
            Ok(Err(reason)) => results.push(BackendProbeResult {
                name: candidate.to_string(),
                available: false,
                reason,
            }),
            Err(_) => results.push(BackendProbeResult {
                name: candidate.to_string(),
                available: false,
                reason: "probe timed out".into(),
            }),
        }
    }

    Err(ComposeError::NoBackendFound { probed: results })
}

fn platform_candidates() -> &'static [&'static str] {
    if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
        &[
            "apple/container",
            "orbstack",
            "colima",
            "rancher-desktop",
            "lima",
            "podman",
            "nerdctl",
            "docker",
        ]
    } else if cfg!(target_os = "linux") {
        &["podman", "nerdctl", "docker"]
    } else {
        // Windows and other platforms
        &["podman", "nerdctl", "docker"]
    }
}

async fn probe_candidate(name: &str) -> std::result::Result<Box<dyn ContainerBackend>, String> {
    let which_bin = |name: &str| -> std::result::Result<PathBuf, String> {
        which::which(name).map_err(|_| format!("{} not found", name))
    };

    match name {
        "apple/container" => {
            let bin = which_bin("container")?;
            Ok(Box::new(CliBackend::new(
                bin,
                Box::new(AppleContainerProtocol),
            )))
        }
        "podman" => {
            let bin = which_bin("podman")?;
            if cfg!(target_os = "macos") {
                let out = Command::new(&bin)
                    .args(&["machine", "list", "--format", "json"])
                    .output()
                    .await
                    .map_err(|_| "podman machine list failed")?;
                let json: serde_json::Value =
                    serde_json::from_slice(&out.stdout).map_err(|_| "invalid podman output")?;
                if !json
                    .as_array()
                    .map(|a| a.iter().any(|m| m["Running"].as_bool().unwrap_or(false)))
                    .unwrap_or(false)
                {
                    return Err("no podman machine running".into());
                }
            }
            Ok(Box::new(CliBackend::new(bin, Box::new(DockerProtocol))))
        }
        "orbstack" => {
            let bin = which_bin("orb")
                .or_else(|_| which_bin("docker"))
                .map_err(|_| "orbstack not found")?;
            Ok(Box::new(CliBackend::new(bin, Box::new(DockerProtocol))))
        }
        "colima" => {
            let bin = which_bin("colima")?;
            let out = Command::new(&bin)
                .arg("status")
                .output()
                .await
                .map_err(|_| "colima status failed")?;
            if !String::from_utf8_lossy(&out.stdout).contains("running") {
                return Err("colima not running".into());
            }
            let dbin = which_bin("docker").map_err(|_| "docker cli not found for colima")?;
            Ok(Box::new(CliBackend::new(dbin, Box::new(DockerProtocol))))
        }
        "lima" => {
            let bin = which_bin("limactl")?;
            let out = Command::new(&bin)
                .args(&["list", "--json"])
                .output()
                .await
                .map_err(|_| "limactl list failed")?;
            let instance = String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .find(|v| v["status"] == "Running")
                .and_then(|v| v["name"].as_str().map(|s| s.to_string()))
                .ok_or("no running lima instance")?;
            Ok(Box::new(CliBackend::new(
                bin,
                Box::new(LimaProtocol { instance }),
            )))
        }
        "nerdctl" => {
            let bin = which_bin("nerdctl")?;
            Ok(Box::new(CliBackend::new(bin, Box::new(DockerProtocol))))
        }
        "docker" => {
            let bin = which_bin("docker")?;
            Ok(Box::new(CliBackend::new(bin, Box::new(DockerProtocol))))
        }
        _ => Err("unknown backend".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ContainerSpec;

    #[test]
    fn test_docker_run_args() {
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "nginx".into(),
            name: Some("web".into()),
            ports: Some(vec!["80:80".into()]),
            env: Some([("FOO".into(), "BAR".into())].into()),
            rm: Some(true),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"--name".to_string()));
        assert!(args.contains(&"web".to_string()));
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"80:80".to_string()));
        assert!(args.contains(&"-e".to_string()));
        assert!(args.contains(&"FOO=BAR".to_string()));
        assert!(args.contains(&"--rm".to_string()));
        assert!(args.contains(&"nginx".to_string()));
    }

    #[test]
    fn test_docker_security_run_args() {
        let proto = DockerProtocol;
        let spec = ContainerSpec {
            image: "nginx".into(),
            privileged: Some(true),
            user: Some("nobody".into()),
            workdir: Some("/tmp".into()),
            cap_add: Some(vec!["NET_ADMIN".into()]),
            cap_drop: Some(vec!["ALL".into()]),
            read_only: Some(true),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        assert!(args.contains(&"--privileged".to_string()));
        assert!(args.contains(&"--user".to_string()));
        assert!(args.contains(&"nobody".to_string()));
        assert!(args.contains(&"--workdir".to_string()));
        assert!(args.contains(&"/tmp".to_string()));
        assert!(args.contains(&"--cap-add".to_string()));
        assert!(args.contains(&"NET_ADMIN".to_string()));
        assert!(args.contains(&"--cap-drop".to_string()));
        assert!(args.contains(&"ALL".to_string()));
        assert!(args.contains(&"--read-only".to_string()));
    }

    #[test]
    fn test_apple_run_args() {
        let proto = AppleContainerProtocol;
        let spec = ContainerSpec {
            image: "alpine".into(),
            rm: Some(true),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        // apple/container run doesn't use --detach by default in our impl
        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"--rm".to_string()));
        assert!(args.contains(&"alpine".to_string()));
        assert!(!args.contains(&"--detach".to_string()));
    }

    #[test]
    fn test_lima_run_args() {
        let proto = LimaProtocol {
            instance: "default".into(),
        };
        let spec = ContainerSpec {
            image: "busybox".into(),
            ..Default::default()
        };

        let args = proto.run_args(&spec);
        assert_eq!(args[0], "shell");
        assert_eq!(args[1], "default");
        assert_eq!(args[2], "nerdctl");
        assert_eq!(args[3], "run");
    }

    #[test]
    fn test_platform_candidates() {
        let candidates = platform_candidates();
        assert!(!candidates.is_empty());
        if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
            assert_eq!(candidates[0], "apple/container");
        } else {
            assert_eq!(candidates[0], "podman");
        }
    }

    #[tokio::test]
    async fn test_detect_backend_env_override() {
        std::env::set_var("PERRY_CONTAINER_BACKEND", "invalid-backend-name");
        let res = detect_backend().await;
        // Clean up before assertion to avoid affecting other tests
        std::env::remove_var("PERRY_CONTAINER_BACKEND");

        assert!(res.is_err());
        if let Err(ComposeError::NoBackendFound { probed }) = res {
            assert_eq!(probed.len(), 1);
            assert_eq!(probed[0].name, "invalid-backend-name");
            assert_eq!(probed[0].reason, "unknown backend");
        } else {
            panic!("Expected NoBackendFound error");
        }
    }
}
