use log::warn;
use std::{
    env, error,
    fs::{metadata, File},
    io::{BufRead, BufReader},
    net, str,
};

const MAX_ENV_FILE_SIZE: u64 = 8 * 1024; // 8 KiB Limit for BufReader

#[derive(Debug, PartialEq, Eq)]
pub enum CompressionType {
    Zstd,
    Gzip,
    None,
}

impl str::FromStr for CompressionType {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "gzip" => Ok(CompressionType::Gzip),
            "zstd" => Ok(CompressionType::Zstd),
            "none" => Ok(CompressionType::None),
            _ => Err("Invalid compression type."),
        }
    }
}

impl Default for CompressionType {
    fn default() -> Self {
        CompressionType::Zstd
    }
}

pub struct Config {
    pub host: String,
    pub port: u16,
    pub compression_type: CompressionType,
    pub reliable: bool,
    pub pem_path: String,
    pub proto_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            host: "127.0.0.1".to_string(),
            port: 50505,
            compression_type: CompressionType::default(),
            reliable: true,
            pem_path: "cert.pem".to_string(),
            proto_path: "message.proto".to_string(),
        }
    }
}

impl Config {
    pub fn from_env(file_path: Option<&str>) -> Result<Self, Box<dyn error::Error>> {
        if let Some(path) = file_path {
            set_env_vars(path)?;
        }

        let host = match env::var("CRUMB_HOST") {
            Ok(value) => {
                let clean_host = from_raw_string(&value);
                if !is_valid_ip(&clean_host) {
                    panic!("Invalid IP address provided for CRUMB_HOST: {}", clean_host);
                }
                clean_host
            }
            Err(e) => {
                let default_host = Config::default().host;
                warn!(
                    "CRUMB_HOST not set or invalid. Defaulting to {}. Error: {}",
                    default_host, e
                );
                default_host
            }
        };

        let port: u16 = get_env_var("CRUMB_PORT");
        let compression_type: CompressionType = get_env_var("CRUMB_COMPRESSION_TYPE");
        let reliable: bool = get_env_var("CRUMB_RELIABLE");
        let proto_path = match env::var("CRUMB_PROTO_PATH") {
            Ok(value) => from_raw_string(&value),
            Err(e) => {
                panic!(
                    "CRUMB_PROTO_PATH not set or invalid. A .proto file is required. Error: {}",
                    e
                );
            }
        };
        let pem_path = match env::var("CRUMB_PEM_PATH") {
            Ok(value) => from_raw_string(&value),
            Err(e) => {
                warn!(
                    "CRUMB_PEM_PATH not set or invalid. Defaulting to cleartext. Error: {}",
                    e
                );
                Default::default()
            }
        };

        let config = Config {
            host,
            port,
            compression_type,
            reliable,
            proto_path,
            pem_path,
        };

        Ok(config)
    }
}

fn is_valid_ip(host: &str) -> bool {
    host.parse::<net::IpAddr>().is_ok()
}

fn get_env_var<T: str::FromStr + Default>(key: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_default()
}

fn set_env_vars(file_path: &str) -> Result<(), Box<dyn error::Error>> {
    let file_size = metadata(file_path)?.len();
    assert!(
        file_size < MAX_ENV_FILE_SIZE,
        ".env file exceeds BufReader::new limit of 8KiB"
    );

    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l.trim().to_string(),
            Err(e) => {
                eprintln!("Skipping unreadable line in '{}': {}", file_path, e);
                continue;
            }
        };

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut in_quotes = false;
        let mut trimmed_line = String::new();

        for c in line.chars() {
            match c {
                '"' | '\'' => in_quotes = !in_quotes,
                '#' if !in_quotes => break,
                _ => trimmed_line.push(c),
            }
        }

        if let Some((key, value)) = trimmed_line.split_once('=') {
            let key = key.trim();
            let value = value.trim();

            if key.is_empty() || value.is_empty() {
                warn!("Skipping invalid ENV line: '{}'", line);
                continue;
            }

            env::set_var(key, value);
        } else {
            warn!("Skipping malformed ENV line: '{}'", line);
        }
    }

    Ok(())
}

fn from_raw_string(input: &str) -> String {
    input
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_file_full() {
        // .test-env-full
        // CRUMB_HOST="1.2.3.4"
        // CRUMB_PORT=55555
        // CRUMB_COMPRESSION_TYPE=gzip
        // CRUMB_RELIABLE=false
        // CRUMB_PEM_PATH="its/just/a/test.pem"
        // CRUMB_PROTO_PATH="testing/tests/stuff.proto"
        let config =
            Config::from_env(Some("/home/neo/repos/crumb/src/util/.test-env-full")).unwrap();
        assert_eq!(config.host, "1.2.3.4".to_owned());
        assert_eq!(config.port, 55555);
        assert_eq!(config.compression_type, CompressionType::Gzip);
        assert_eq!(config.reliable, false);
        assert_eq!(config.pem_path, "its/just/a/test.pem".to_owned());
        assert_eq!(config.proto_path, "testing/tests/stuff.proto".to_owned());

        // Cleanup env vars
        env::remove_var("CRUMB_HOST");
        env::remove_var("CRUMB_PORT");
        env::remove_var("CRUMB_COMPRESSION_TYPE");
        env::remove_var("CRUMB_RELIABLE");
        env::remove_var("CRUMB_PEM_PATH");
        env::remove_var("CRUMB_PROTO_PATH");
    }

    #[test]
    #[should_panic(expected = "Invalid IP address provided for CRUMB_HOST: 1234")]
    fn env_file_full_bad() {
        // .test-env-full-bad
        // CRUMB_HOST=1234
        // CRUMB_PORT="woops"
        // CRUMB_COMPRESSION_TYPE=gzipper
        // CRUMB_RELIABLE=farse
        // CRUMB_PEM_PATH=1
        // CRUMB_PROTO_PATH=1000
        let config =
            Config::from_env(Some("/home/neo/repos/crumb/src/util/.test-env-full-bad")).unwrap();
        assert_eq!(config.host, "127.0.0.1".to_owned());
        assert_eq!(config.port, 50505);
        assert_eq!(config.compression_type, CompressionType::Zstd);
        assert_eq!(config.reliable, true);
        assert_eq!(config.pem_path, "cert.pem".to_owned());
        assert_eq!(config.proto_path, "message.proto".to_owned());

        // Cleanup env vars
        env::remove_var("CRUMB_HOST");
        env::remove_var("CRUMB_PORT");
        env::remove_var("CRUMB_COMPRESSION_TYPE");
        env::remove_var("CRUMB_RELIABLE");
        env::remove_var("CRUMB_PEM_PATH");
        env::remove_var("CRUMB_PROTO_PATH");
    }

    #[test]
    fn env_file_full_with_comments() {
        // .test-env-full-comments
        // # This is a comment
        // CRUMB_HOST="1.2.3.4" # This is also a comment
        // CRUMB_PORT=55555
        // CRUMB_COMPRESSION_TYPE=gzip
        // CRUMB_RELIABLE=false
        // # The comment in the path should be ignored
        // CRUMB_PEM_PATH="#its/just/a/test.pem" # And so should this "#"
        // CRUMB_PROTO_PATH="testing/tests/stuff.proto"
        let config =
            Config::from_env(Some("/home/neo/repos/crumb/src/util/.test-env-full")).unwrap();
        assert_eq!(config.host, "1.2.3.4".to_owned());
        assert_eq!(config.port, 55555);
        assert_eq!(config.compression_type, CompressionType::Gzip);
        assert_eq!(config.reliable, false);
        assert_eq!(config.pem_path, "its/just/a/test.pem".to_owned());
        assert_eq!(config.proto_path, "testing/tests/stuff.proto".to_owned());

        // Cleanup env vars
        env::remove_var("CRUMB_HOST");
        env::remove_var("CRUMB_PORT");
        env::remove_var("CRUMB_COMPRESSION_TYPE");
        env::remove_var("CRUMB_RELIABLE");
        env::remove_var("CRUMB_PEM_PATH");
        env::remove_var("CRUMB_PROTO_PATH");
    }

    #[test]
    #[should_panic(
        expected = "CRUMB_PROTO_PATH not set or invalid. A .proto file is required. Error: environment variable not found"
    )]
    // Config {
    //             host: "[::]".to_string(),
    //             port: 50505,
    //             compression_type: CompressionType::default(),
    //             reliable: true,
    //             pem_path: "cert.pem".to_string(),
    //             proto_path: "message.proto".to_string(),
    //         }

    fn env_file_empty() {
        let config =
            Config::from_env(Some("/home/neo/repos/crumb/src/util/.test-env-empty")).unwrap();
        assert_eq!(config.host, "127.0.0.1".to_string());
        assert_eq!(config.port, 50505);
        assert_eq!(config.compression_type, CompressionType::Zstd);
        assert_eq!(config.reliable, true);
        assert_eq!(config.pem_path, "cert.pem".to_string());
        assert_eq!(config.proto_path, "message.proto".to_string());
    }

    #[test]
    #[should_panic(
        expected = "CRUMB_PROTO_PATH not set or invalid. A .proto file is required. Error: environment variable not found"
    )]
    fn env_file_missing() {
        let config = Config::from_env(None).unwrap();
        assert_eq!(config.host, "127.0.0.1".to_string());
        assert_eq!(config.port, 50505);
        assert_eq!(config.compression_type, CompressionType::Zstd);
        assert_eq!(config.reliable, true);
        assert_eq!(config.pem_path, "cert.pem".to_string());
        assert_eq!(config.proto_path, "message.proto".to_string());
    }

    #[test]
    fn raw_empty_string() {
        let raw = from_raw_string(r#""#);
        assert_eq!("", raw);
    }

    #[test]
    fn raw_empty_string_with_double_quotes() {
        let not_raw = from_raw_string("\"\"");
        assert_eq!("", not_raw);
    }

    #[test]
    fn raw_empty_string_with_single_quotes() {
        let not_raw = from_raw_string("\'\'");
        assert_eq!("", not_raw);
    }

    #[test]
    fn raw_empty_string_with_many_double_quotes() {
        let raw = from_raw_string(r#"s"t"u"ff"""#);
        assert_ne!("stuff", raw);
    }

    #[test]
    fn raw_empty_string_with_many_single_quotes() {
        let raw = from_raw_string(r#"s't'u'ff''"#);
        assert_ne!("stuff", raw);
    }

    #[test]
    fn compression_type_from_str() {
        assert_eq!(CompressionType::Zstd, "zstd".parse().unwrap());
        assert_eq!(CompressionType::Gzip, "gzip".parse().unwrap());
        assert_eq!(CompressionType::None, "none".parse().unwrap());
        assert_eq!(CompressionType::Zstd, "ZSTD".parse().unwrap());
        assert_eq!(CompressionType::Gzip, "GZIP".parse().unwrap());
        assert_eq!(CompressionType::None, "NONE".parse().unwrap());
    }

    #[test]
    #[should_panic(
        expected = "called `Result::unwrap()` on an `Err` value: \"Invalid compression type.\""
    )]
    fn bad_compression_type_from_str() {
        assert_eq!(CompressionType::Zstd, "".parse().unwrap());
    }

    #[test]
    fn good_ipv4() {
        assert_eq!(true, is_valid_ip("127.0.0.1"))
    }

    #[test]
    fn good_ipv6() {
        assert_eq!(true, is_valid_ip("::1"))
    }

    #[test]
    fn bad_ipv4() {
        assert_eq!(false, is_valid_ip("127.0.0"))
    }

    #[test]
    fn bad_ipv6() {
        assert_eq!(false, is_valid_ip(":1"))
    }
}
