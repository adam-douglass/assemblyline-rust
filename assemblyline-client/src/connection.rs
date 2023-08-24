
use std::collections::HashMap;

use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use serde_json::{json, Value};

use crate::types::{Authentication, JsonMap, Error};


/// A connection abstraction to handle queries
pub (crate) struct Connection {
    client: reqwest::Client,
    server: String,
    max_retries: Option<u32>,
    authentication: Authentication,
//     self.debug = debug
//     self.is_v4 = False
//     self.silence_warnings = silence_warnings
    _verify: bool,
    default_timeout: Option<f64>,

    session_header_label: reqwest::header::HeaderName,
    session_token: tokio::sync::RwLock<Option<reqwest::header::HeaderValue>>,
}

impl Connection {
    /// Connect to an assemblyline system
    pub async fn connect(
        server: String,
        auth: Authentication,
        retry: Option<u32>,
        verify: bool,
        raw_headers: HashMap<String, String>,
        cert: Option<String>,
        timeout: Option<f64>
    ) -> Result<Self, Error> {
        let mut builder = if verify {
            reqwest::Client::builder()
        } else {
            reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
        };

        // insert certificate
        if let Some(cert) = cert {
            builder = builder.add_root_certificate(reqwest::Certificate::from_pem(cert.as_bytes())?);
        }

        // build headers
        let mut headers = HeaderMap::new();
        for (name, value) in raw_headers.into_iter() {
            let name: reqwest::header::HeaderName = name.parse()?;
            headers.insert(name, value.parse()?);
        }

        // finalize client
        let client = builder
            .cookie_store(true)
            .default_headers(headers)
            .build()?;

        let con = Connection {
            client,
            server,
            max_retries: retry,
            authentication: auth,
            _verify: verify,
            default_timeout: timeout,
            session_header_label: reqwest::header::HeaderName::from_lowercase(b"x-xsrf-token")?,
            session_token: tokio::sync::RwLock::new(None),
        };

        // check API version
        let versions = con.get("api/", convert_api_output_list).await?;
        let found = versions.into_iter()
            .map(|version| match version.as_str() {None => false, Some(version) => version == "v4"})
            .any(|b|b);
        if !found {
            return Err(Error::client_error("Supported APIS (v4) are not available".to_owned(), 400))
        }

        // Login
        let _auth_details = con.authenticate().await?;
        // session.timeout = auth_session_detail['session_duration']

        return Ok(con)
    }

    /// Login to the assemblyline system
    #[async_recursion::async_recursion]
    async fn authenticate(&self) -> Result<JsonMap, Error> {
        let body = match &self.authentication {
            Authentication::Password { username, password } => {
                json!({
                    "user": username,
                    "password": password,
                })
            },
            Authentication::ApiKey { username, key } => {
                json!({
                    "user": username,
                    "apikey": key,
                })
            },
            Authentication::OAuth { provider, token } => {
                json!({
                    "oauth_provider": provider,
                    "oauth_token": token
                })
            },
        };

        return self.get_with("api/v4/auth/login/", Some(body), convert_api_output_map).await;
    }

// def delete(self, path, **kw):
//     return self.request(self.session.delete, path, convert_api_output, **kw)

// def download(self, path, process, **kw):
//     return self.request(self.session.get, path, process, **kw)

    // pub fn get(self: &Arc<Self>, path: &str) -> RequestBuilder {
    //     // return self.request(self.session.get, path, convert_api_output, **kw)
    //     todo!()
    // }

    pub async fn get<Resp, F>(&self, path: &str, con: F) -> Result<Resp, Error>
        where F: Fn(JsonMap) -> Result<Resp, Error>
    {
        let resp = self.request::<()>(reqwest::Method::GET, path, None, None).await?;
        let body: JsonMap = resp.json().await?;
        return con(body)
    }

    pub async fn get_with<Req, Resp, F>(&self, path: &str, body: Req, con: F) -> Result<Resp, Error>
        where Req: serde::Serialize,
              F: Fn(JsonMap) -> Result<Resp, Error>
    {
        let resp = self.request(reqwest::Method::GET, path, Some(body), None).await?;
        let body: JsonMap = resp.json().await?;
        return con(body)
    }

    pub async fn post<Req, Resp, F>(&self, path: &str, body: Req, con: F) -> Result<Resp, Error>
        where Req: serde::Serialize,
              F: Fn(JsonMap) -> Result<Resp, Error>
    {
        let resp = self.request(reqwest::Method::POST, path, Some(body), None).await?;
        let body: JsonMap = resp.json().await?;
        return con(body)
    }

// def put(self, path, **kw):
//     return self.request(self.session.put, path, convert_api_output, **kw)

    /// Detailed method to make an http request
    pub async fn request<Req>(&self, method: reqwest::Method, path: &str, body: Option<Req>, timeout: Option<f64>) -> Result<reqwest::Response, Error>
        where Req: serde::Serialize
    {
        // Apply default timeout parameter if not passed elsewhere
        let timeout = match timeout {
            Some(time) => Some(time),
            None => self.default_timeout,
        };

        let mut retries = 0;
        while self.max_retries.map_or(true, |max| retries <= max) {
            if retries > 0 {
                let seconds = 2.0_f64.min(2.0_f64.powf(retries as f64 - 7.0));
                tokio::time::sleep(tokio::time::Duration::from_secs_f64(seconds)).await;
                // stream = kw.get('files', {}).get('bin', None)
                // if stream and 'seek' in dir(stream):
                //     stream.seek(0)
            }

            // response = func('/'.join((self.server, path)), **kw)
            let url = format!("{}/{}", self.server, path);
            let mut request = self.client.request(method.clone(), url);


            if let Some(body) = &body {
                request = request.json(body);
            }

            // set timeout
            if let Some(timeout) = timeout {
                request = request.timeout(std::time::Duration::from_secs_f64(timeout));
            }

            // attach the session header
            if let Some(token) = self.session_token.read().await.as_ref() {
                request = request.header(self.session_header_label.clone(), token);
            }

            // let request = match body {
            //     Body::None => request,
            //     Body::Json(body) => request.json(&body),
            // };

            // issue the request
            retries += 1;
            let response = match request.send().await {
                Ok(response) => response,
                Err(err) => {
                    // for connection errors continue, previously this would not include
                    // ssl or proxy errors
                    if err.is_connect() {
                        continue
                    }

                    // for other non-http errors break the loop
                    return Err(err.into())
                }
            };

            // Capture session cookie
            for cookie in response.cookies() {
                if cookie.name() == "XSRF-TOKEN" {
                    *self.session_token.write().await = Some(cookie.value().parse()?)
                }
            }

            let status = response.status();
            if status.is_success() {
                return Ok(response)
            }

            if status == StatusCode::UNAUTHORIZED {
                let body = response.text().await?;
                if let Ok(resp) = serde_json::from_str::<Value>(&body) {
                    if let Some(resp) = resp.as_object() {
                        if let Some(error) = resp.get("api_error_message") {
                            let error = error.as_str().map(|s|s.to_string()).unwrap_or(error.to_string());
                            if is_session_error(&error) {
                                self.authenticate().await?;
                                continue;
                            }

                            return Err(Error::Client {
                                message: error,
                                status: status.as_u16() as u32,
                                api_version: resp.get("api_server_version").map(|ver| ver.as_str().map(|ver| ver.to_string()).unwrap_or(ver.to_string())),
                                api_response: resp.get("api_response").map(|ver| ver.as_str().map(|ver| ver.to_string()).unwrap_or(ver.to_string()))
                            })
                        }
                    }
                }

                return Err(Error::client_error(body, status.as_u16() as u32));

            } else if status == StatusCode::BAD_GATEWAY || status == StatusCode::SERVICE_UNAVAILABLE || status == StatusCode::GATEWAY_TIMEOUT {
                let body = response.text().await?;
                if let Ok(resp) = serde_json::from_str::<Value>(&body) {
                    if let Some(resp) = resp.as_object() {
                        return Err(Error::Client {
                            message: resp.get("api_error_message").map(|ver| ver.as_str().map(|ver| ver.to_string()).unwrap_or(ver.to_string())).unwrap_or("unknown error".to_owned()),
                            status: status.as_u16() as u32,
                            api_version: resp.get("api_server_version").map(|ver| ver.as_str().map(|ver| ver.to_string()).unwrap_or(ver.to_string())),
                            api_response: resp.get("api_response").map(|ver| ver.as_str().map(|ver| ver.to_string()).unwrap_or(ver.to_string()))
                        })
                    }
                }
                return Err(Error::client_error(body, status.as_u16() as u32));
            }
        }

        return Err(Error::client_error("Max retry reached, could not perform the request.".to_owned(), 429))
    }
}

// pub struct RequestBuilder {

// }

// impl RequestBuilder {
//     pub async fn send(self) -> Result<reqwest::Response, Error> {
//         todo!()
//     }
// }

// enum Body<T> {
//     None,
//     Json(T),
// }


fn is_session_error(error: &str) -> bool {
    matches!(error,
        "Session rejected" |
        "Session not found" |
        "Session expired" |
        "Invalid source IP for this session" |
        "Invalid user agent for this session"
    )
}

pub fn convert_api_output_string(mut obj: JsonMap) -> Result<String, Error> {
    if let Some(Value::String(string)) = obj.remove("api_response") {
        return Ok(string)
    }
    return Err(Error::MalformedResponse)
}

pub fn convert_api_output_map(mut obj: JsonMap) -> Result<JsonMap, Error> {
    if let Some(Value::Object(map)) = obj.remove("api_response") {
        return Ok(map)
    }
    return Err(Error::MalformedResponse)
}

pub fn convert_api_output_list(mut obj: JsonMap) -> Result<Vec<Value>, Error> {
    if let Some(Value::Array(values)) = obj.remove("api_response") {
        return Ok(values)
    }
    return Err(Error::MalformedResponse)
}