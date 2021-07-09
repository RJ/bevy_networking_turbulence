// Arg parser for the examples.
// 
// each example gets a different Args struct, which it adds as a bevy resource

use clap::{Arg, App as ClapApp}; //, value_t_or_exit};

#[derive(Debug)]
pub struct SimpleArgs {
    pub is_server: bool,
}

fn exe_name() -> String {
    match std::env::current_exe() {
        Ok(pathbuf) => match pathbuf.file_name() {
            Some(name) => name.to_string_lossy().into(),
            None => String::new()
        },
        Err(_) => String::new()
    }
}

#[allow(dead_code)]
pub fn parse_simple_args() -> SimpleArgs {
    let matches = ClapApp::new(exe_name())
        .about("Simple example just sends some packets")
        .args(server_or_client_args().as_slice())
        .get_matches();
    SimpleArgs {
        is_server: matches.is_present("server")
    }
}

fn server_or_client_args<'a>() -> Vec<Arg<'a, 'a>> {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            // will default to client, unless server specified.
            // wasm32 builds can't be a server.
            vec![]
        } else {
            vec![
                Arg::with_name("server")
                .help("Listen as a server")
                .long("server")
                .required_unless("client")
                .conflicts_with("client")
                .takes_value(false)
                ,
                Arg::with_name("client")
                .help("Connect as a client")
                .long("client")
                .required_unless("server")
                .conflicts_with("server")
                .takes_value(false)
            ]
        }
    }
}

