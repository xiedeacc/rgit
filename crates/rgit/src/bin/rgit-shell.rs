use std::path::PathBuf;

fn main() {
    let (config, key_id) = match arguments() {
        Ok(arguments) => arguments,
        Err(message) => {
            eprintln!("rgit: {message}");
            std::process::exit(1);
        }
    };
    let original = match std::env::var("SSH_ORIGINAL_COMMAND") {
        Ok(command) => command,
        Err(_) => {
            eprintln!("rgit: only Git commands are supported");
            std::process::exit(1);
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build runtime");
    match runtime.block_on(rgit_ssh::shell::run(&config, key_id, &original)) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("rgit: {error}");
            std::process::exit(1);
        }
    }
}

fn arguments() -> Result<(PathBuf, i64), &'static str> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--config")) {
        return Err("invalid forced command");
    }
    let config = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("invalid forced command")?;
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--key-id")) {
        return Err("invalid forced command");
    }
    let key_id = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or("invalid forced command")?;
    if arguments.next().is_some() {
        return Err("invalid forced command");
    }
    Ok((config, key_id))
}
