//! ZITADEL gRPC 客户端共享连接与凭据能力。

use std::{fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use grpc::{
    StatusError,
    client::{Channel, ChannelOptions},
    credentials::{
        CompositeChannelCredentials, LocalChannelCredentials, SecurityLevel,
        call::{CallCredentials, CallDetails, ClientConnectionSecurityInfo},
        rustls::client::{ClientTlsConfig, RustlsChannelCredendials},
    },
    metadata::{AsciiMetadataValue, MetadataMap},
};
use thiserror::Error;
use url::{Host, Url};

/// ZITADEL 管理 API 单次 gRPC 请求超时时间。
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// 创建 ZITADEL gRPC channel 时发现的本地配置错误。
#[derive(Debug, Error)]
pub(crate) enum ClientError {
    /// issuer、PAT 或 gRPC metadata 配置不满足安全约束。
    #[error("ZITADEL gRPC 客户端配置无效: {0}")]
    InvalidConfiguration(&'static str),
    /// TLS 凭据无法使用系统证书库创建。
    #[error("ZITADEL gRPC TLS 配置无效: {0}")]
    TlsConfiguration(String),
}

/// 使用 OIDC issuer 与 PAT 创建可调用 ZITADEL v2 管理 API 的 gRPC channel。
pub(crate) fn authenticated_channel(
    issuer: &str,
    personal_access_token: &str,
) -> Result<Channel, ClientError> {
    let endpoint = grpc_endpoint(issuer)?;
    let authorization = authorization_value(personal_access_token)?;
    let call_credentials = Arc::new(PatCallCredentials { authorization });
    let channel = if endpoint.secure {
        _ = rustls::crypto::ring::default_provider().install_default();
        let tls = RustlsChannelCredendials::new(ClientTlsConfig::new())
            .map_err(ClientError::TlsConfiguration)?;
        Channel::new(
            endpoint.target,
            Arc::new(CompositeChannelCredentials::new(tls, call_credentials)),
            ChannelOptions::default(),
        )
    } else {
        Channel::new(
            endpoint.target,
            Arc::new(CompositeChannelCredentials::new(
                LocalChannelCredentials::new(),
                call_credentials,
            )),
            ChannelOptions::default(),
        )
    };
    Ok(channel)
}

#[derive(Clone)]
struct GrpcEndpoint {
    target: String,
    secure: bool,
}

#[derive(Clone)]
struct PatCallCredentials {
    authorization: AsciiMetadataValue,
}

impl fmt::Debug for PatCallCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PatCallCredentials")
            .field("authorization", &"[REDACTED]")
            .finish()
    }
}

#[async_trait]
impl CallCredentials for PatCallCredentials {
    async fn get_metadata(
        &self,
        _call_details: &CallDetails,
        _auth_info: &ClientConnectionSecurityInfo,
        metadata: &mut MetadataMap,
    ) -> Result<(), StatusError> {
        metadata.insert("authorization", self.authorization.clone());
        Ok(())
    }

    fn minimum_channel_security_level(&self) -> SecurityLevel {
        SecurityLevel::NoSecurity
    }
}

fn grpc_endpoint(issuer: &str) -> Result<GrpcEndpoint, ClientError> {
    let url = Url::parse(issuer.trim())
        .map_err(|_| ClientError::InvalidConfiguration("OIDC issuer URL 无效"))?;
    if url.host().is_none() {
        return Err(ClientError::InvalidConfiguration(
            "OIDC issuer 必须是包含主机的绝对 URL",
        ));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ClientError::InvalidConfiguration(
            "OIDC issuer 不能包含凭据、query 或 fragment",
        ));
    }
    let secure = match url.scheme() {
        "https" => true,
        "http" if is_loopback(&url) => false,
        _ => {
            return Err(ClientError::InvalidConfiguration(
                "OIDC issuer 必须使用 HTTPS；仅 loopback 开发地址允许 HTTP",
            ));
        }
    };
    let host = url
        .host_str()
        .ok_or(ClientError::InvalidConfiguration("OIDC issuer 缺少主机"))?;
    let port = url
        .port_or_known_default()
        .ok_or(ClientError::InvalidConfiguration("OIDC issuer 缺少端口"))?;
    let authority = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    Ok(GrpcEndpoint {
        target: format!("dns:///{authority}"),
        secure,
    })
}

fn authorization_value(personal_access_token: &str) -> Result<AsciiMetadataValue, ClientError> {
    let token = personal_access_token.trim();
    if token.is_empty() {
        return Err(ClientError::InvalidConfiguration(
            "Personal Access Token 不能为空",
        ));
    }
    let mut authorization = AsciiMetadataValue::try_from(format!("Bearer {token}").as_bytes())
        .map_err(|_| {
            ClientError::InvalidConfiguration("Personal Access Token 包含非法 metadata 字符")
        })?;
    authorization.set_sensitive(true);
    Ok(authorization)
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}
