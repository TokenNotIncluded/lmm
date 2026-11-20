//! Preserve Mozilla roots when adapting reqwest 0.12 clients to 0.13.
pub(crate) fn builder() -> Result<reqwest::blocking::ClientBuilder, reqwest::Error> {
    let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .map(|certificate| reqwest::Certificate::from_der(certificate.as_ref()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(reqwest::blocking::Client::builder().tls_certs_only(roots))
}
