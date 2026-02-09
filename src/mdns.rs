//! mDNS/Bonjour service discovery for local network auto-discovery.
//!
//! Announces the yocore HTTP API server on the local network so desktop apps
//! can find all running instances without manual configuration.
//!
//! Service type: `_yocore._tcp.local.`
//! TXT records: version, uuid, hostname, api_key_required, projects

use mdns_sd::{ServiceDaemon, ServiceInfo};

const SERVICE_TYPE: &str = "_yocore._tcp.local.";

/// Handle to a registered mDNS service. Unregisters on drop or explicit call.
pub struct MdnsService {
    daemon: ServiceDaemon,
    fullname: String,
}

/// Metadata advertised in mDNS TXT records.
pub struct MdnsMetadata {
    pub version: String,
    pub uuid: String,
    pub hostname: String,
    pub api_key_required: bool,
    pub project_count: usize,
}

impl MdnsService {
    /// Register the yocore service via mDNS on all network interfaces.
    pub fn register(
        instance_name: &str,
        port: u16,
        metadata: MdnsMetadata,
    ) -> Result<Self, String> {
        let daemon =
            ServiceDaemon::new().map_err(|e| format!("Failed to create mDNS daemon: {}", e))?;

        let properties = [
            ("version", metadata.version.as_str()),
            ("uuid", metadata.uuid.as_str()),
            ("hostname", metadata.hostname.as_str()),
            (
                "api_key_required",
                if metadata.api_key_required {
                    "true"
                } else {
                    "false"
                },
            ),
            // project_count is converted to a leaked &str for the property slice lifetime
        ];

        let project_count_str = metadata.project_count.to_string();
        let mut props: Vec<(&str, &str)> = properties.to_vec();
        props.push(("projects", &project_count_str));

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            instance_name,
            &format!("{}.local.", metadata.hostname),
            "",
            port,
            props.as_slice(),
        )
        .map_err(|e| format!("Failed to create service info: {}", e))?
        .enable_addr_auto();

        let fullname = service_info.get_fullname().to_string();

        daemon
            .register(service_info)
            .map_err(|e| format!("Failed to register mDNS service: {}", e))?;

        tracing::info!(
            "mDNS service registered: {} on port {}",
            instance_name,
            port
        );

        Ok(MdnsService { daemon, fullname })
    }

    /// Unregister the service (called on shutdown).
    pub fn unregister(&self) {
        if let Err(e) = self.daemon.unregister(&self.fullname) {
            tracing::warn!("Failed to unregister mDNS service: {}", e);
        } else {
            tracing::info!("mDNS service unregistered");
        }
    }
}

impl Drop for MdnsService {
    fn drop(&mut self) {
        self.unregister();
    }
}

/// Generate an instance name for mDNS announcement.
/// Uses custom name if provided, otherwise "Yocore-{hostname}-{short_uuid}".
pub fn generate_instance_name(hostname: &str, uuid: &str, custom_name: Option<&str>) -> String {
    if let Some(name) = custom_name {
        return name.to_string();
    }
    let short_uuid = &uuid[..8.min(uuid.len())];
    format!("Yocore-{}-{}", hostname, short_uuid)
}
