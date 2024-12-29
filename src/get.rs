use crate::{
    cli::ConfOutputType,
    config::{Blacklist, Config, Data, Graylist, Init, Whitelist},
};

pub fn get_default_config(o: ConfOutputType) -> Result<(), anyhow::Error> {
    if o.formatted.is_some() {
        println!("{}", Config::default());
    } else if o.pretty.is_some() {
        println!("{}", serde_json::to_string_pretty(&Config::default())?);
    } else if o.json.is_some() {
        println!("{}", serde_json::to_string(&Config::default())?);
    } else {
        println!("{}", Config::default());
    }

    Ok(())
}

pub fn get_example_config(o: ConfOutputType) -> Result<(), anyhow::Error> {
    let config = Config {
        init: Some(Init {
            name: Some(String::from("Example")),
            hostname: Some(String::from("100.0.0.10")),
            port: Some(22),
            username: Some(String::from("bobthebuilder")),
            iface: Some(String::from("eth0")),
            prog_type: Some(String::from("ip")),
            whitelist: Some(Whitelist {
                enabled: Some(true),
                max: Some(32),
                action: Some(String::from("allow")),
            }),
            blacklist: Some(Blacklist {
                enabled: Some(true),
                max: Some(32),
                action: Some(String::from("deny")),
            }),
            graylist: Some(Graylist {
                enabled: Some(true),
                max: Some(32),
                action: Some(String::from("investigate")),
                frequency: Some(1000),
                fast_packet_count: Some(10),
            }),
        }),
        data: Some(Data {
            whitelist: Some(vec![String::from("192.168.1.103")]),
            blacklist: Some(vec![String::from("192.168.1.203")]),
            graylist: Some(Vec::new()),
        }),
    };

    if o.pretty.is_some() {
        println!("{}", serde_json::to_string_pretty(&config)?);
    } else if o.json.is_some() {
        println!("{}", serde_json::to_string(&config)?);
    } else if o.formatted.is_some() {
        println!("{}", &config);
    } else {
        println!("{}", serde_json::to_string_pretty(&config)?);
    }

    Ok(())
}

pub fn get_base_config(o: ConfOutputType) -> Result<(), anyhow::Error> {
    let config = Config {
        init: Some(Init {
            name: Some(String::from("MyFirstProgram")),
            hostname: None,
            port: None,
            username: None,
            iface: Some(String::from("lo")),
            prog_type: Some(String::from("ip")),
            whitelist: Some(Whitelist {
                enabled: Some(true),
                max: Some(32),
                action: Some(String::from("allow")),
            }),
            blacklist: Some(Blacklist {
                enabled: Some(true),
                max: Some(32),
                action: Some(String::from("deny")),
            }),
            graylist: None,
        }),
        data: Some(Data {
            whitelist: Some(vec![]),
            blacklist: Some(vec![]),
            graylist: None,
        }),
    };

    if o.pretty.is_some() {
        println!("{}", serde_json::to_string_pretty(&config)?);
    } else if o.json.is_some() {
        println!("{}", serde_json::to_string(&config)?);
    } else if o.formatted.is_some() {
        println!("{}", &config);
    } else {
        println!("{}", serde_json::to_string_pretty(&config)?);
    }

    Ok(())
}