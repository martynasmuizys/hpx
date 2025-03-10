use std::cell::SyncUnsafeCell;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::sync::{Arc, Mutex};

use anyhow::anyhow;
use crossterm::style::{style, Stylize};
use ssh2::Session;

use crate::cli::Analyze;
use crate::config::Config;
use crate::SSH_PASS;

static MIN_KERNEL_VERSION: &str = "5.17.0";
static mut UBUNTU_PACKAGES: [&str; 5] = [
    "linux-tools-common",
    "linux-tools-generic",
    "ripgrep",
    "clang",
    "libbpf-dev"
];
static ARCH_PACKAGES: [&str; 6] = ["bpf", "libbpf", "base", "base-devel", "ripgrep", "clang"];

static MISSING_PACKAGES: SyncUnsafeCell<Mutex<Vec<&str>>> =
    SyncUnsafeCell::new(Mutex::new(Vec::new()));

pub async fn analyze(options: Analyze, config: Config) -> Result<bool, anyhow::Error> {
    let mut total_errors = 0;
    let mut error_messages: Vec<anyhow::Error> = Vec::new();
    let mut skip_flag_check = false;

    if options.noconfirm.is_none() {
        println!("{}\n", "- CONFIG -".on_blue().black());
        let mut action = String::new();

        println!("{}", config);

        print!(
            "{}: Using config above. Proceed? [Y/n] ",
            "Analyze".blue().bold()
        );
        io::stdout().flush()?;
        io::stdin().read_line(&mut action)?;

        let action = action.trim().to_lowercase();

        if action != "y" && action != "yes" && !action.is_empty() {
            return Err(anyhow!("Cancelled"));
        }
    }

    let hostname = config.init.as_ref().unwrap().hostname.as_ref();
    let port = config.init.as_ref().unwrap().port.as_ref();

    if hostname.is_none()
        || *hostname.as_ref().unwrap() == "localhost"
        || *hostname.as_ref().unwrap() == "127.0.0.1"
    {
        let mut output =
            String::from_utf8(Command::new("uname").arg("-r").output().unwrap().stdout)?;
        println!("{}", "- Kernel Version Check -".on_blue().black());
        println!(
            "{}: Kernel version: {}",
            "Analyze".blue().bold(),
            &output.trim()
        );
        match check_kernel_version(&output) {
            Ok(_) => (),
            Err(e) => {
                total_errors += 1;
                error_messages.push(e);
            }
        };

        println!("{}", "- Required Packages Check -".on_blue().black());
        output = String::from_utf8(Command::new("uname").arg("-n").output().unwrap().stdout)?;

        match check_packages(options, output.trim()).await {
            Ok(_) => (),
            Err(e) => {
                unsafe {
                    let pkgs = MISSING_PACKAGES.get().as_mut().unwrap();
                    skip_flag_check = pkgs.lock().unwrap().contains(&"ripgrep");
                }
                total_errors += 1;
                error_messages.push(e);
            }
        };

        println!("{}", "- Kernel Flags Check -".on_blue().black());

        if !skip_flag_check {
            output = String::from_utf8(
                Command::new("sh")
                    .args(["-c", "sudo bpftool feature | rg -w 'CONFIG_BPF|CONFIG_BPF_SYSCALL|CONFIG_BPF_JIT|CONFIG_BPF_EVENTS'"])
                    .output()
                    .unwrap()
                .stdout,
            )?;
            match check_bpf_enabled(output.trim().split("\n").collect()) {
                Ok(_) => (),
                Err(e) => {
                    total_errors += 1;
                    error_messages.push(e);
                }
            };
        } else {
            println!(
                "{}: Required kernel flags {}\n",
                "Analyze".blue().bold(),
                "(not ok)".red().bold()
            );
            error_messages.push(anyhow!(
                "Cannot check kernel flags without {} package",
                "ripgrep".bold()
            ));
            total_errors += 1;
        }

        println!("{}", "- Network Interface Check -".on_blue().black());
        output = String::from_utf8(
            Command::new("sh")
                .args(["-c", "ip -o link show | awk -F': ' '{{print $2}}'"])
                .output()
                .unwrap()
                .stdout,
        )?;
        match check_net_iface(
            config.init.as_ref().unwrap().iface.as_ref().unwrap(),
            output.split("\n").collect(),
        ) {
            Ok(_) => (),
            Err(e) => {
                total_errors += 1;
                error_messages.push(e);
            }
        };
    } else if hostname.is_some() {
        let tcp =
            TcpStream::connect(format!("{}:{}", hostname.unwrap(), port.unwrap_or(&22))).unwrap();
        let mut session = Session::new().unwrap();
        session.set_tcp_stream(tcp);
        session.handshake().unwrap();

        let mut username: String = String::new();
        if config.init.as_ref().unwrap().username.is_none() {
            print!("Username: ");
            io::stdout().flush()?;
            io::stdin().read_line(&mut username)?;
        } else {
            username = config
                .init
                .as_ref()
                .unwrap()
                .username
                .as_ref()
                .unwrap()
                .to_string();
            println!(
                "{}: Using username \"{}\"",
                "Analyze".blue().bold(),
                &username
            );
        }

        let password: String;
        unsafe {
            let pass = (*SSH_PASS.get()).lock().unwrap();
            if !pass.is_empty() {
                password = (*pass).clone();
            } else {
                password = rpassword::prompt_password("Password: ")?;
            }
        }

        session.userauth_password(username.trim(), password.trim())?;

        unsafe {
            let pass = (*SSH_PASS.get()).lock().unwrap();
            if pass.is_empty() {
                let new = SSH_PASS.get().as_mut().unwrap();
                *new = Mutex::new(password.clone());
            }
        }

        println!(
            "{}: Connected to {}\n",
            "Analyze".blue().bold(),
            hostname.unwrap()
        );

        println!("{}", "- Kernel Version Check -".on_blue().black());
        match check_kernel_version_remote(&mut session) {
            Ok(_) => (),
            Err(e) => {
                total_errors += 1;
                error_messages.push(e);
            }
        };

        println!("{}", "- Required Packages Check -".on_blue().black());
        match check_packages_remote(options, &mut session, &password) {
            Ok(_) => (),
            Err(e) => {
                unsafe {
                    let pkgs = MISSING_PACKAGES.get().as_mut().unwrap();
                    skip_flag_check = pkgs.lock().unwrap().contains(&"ripgrep");
                }
                total_errors += 1;
                error_messages.push(e);
            }
        };

        println!("{}", "- Kernel Flags Check -".on_blue().black());
        if !skip_flag_check {
            match check_bpf_enabled_remote(&mut session, &password) {
                Ok(_) => (),
                Err(e) => {
                    total_errors += 1;
                    error_messages.push(e);
                }
            };
        } else {
            println!(
                "{}: Required kernel flags {}\n",
                "Analyze".blue().bold(),
                "(not ok)".red().bold()
            );
            error_messages.push(anyhow!(
                "Cannot check kernel flags without {} package",
                "ripgrep".bold()
            ));
            total_errors += 1;
        }

        println!("{}", "- Network Interface Check -".on_blue().black());
        match check_net_iface_remote(
            &mut session,
            config.init.as_ref().unwrap().iface.as_ref().unwrap(),
        ) {
            Ok(_) => (),
            Err(e) => {
                total_errors += 1;
                error_messages.push(e);
            }
        };
    }

    let total_errors_display;
    if total_errors != 0 {
        total_errors_display = style(total_errors).red().bold();
        println!("{}", "- Error List -".on_red().black());
    } else {
        total_errors_display = style(total_errors).red().bold();
    }

    for e in error_messages {
        println!("{}: {}", "Analyze Error".red().bold(), e);
    }

    println!(
        "\n{}: Operating system analysis complete.",
        "Analyze".blue().bold(),
    );
    println!(
        "{}: Total errors: {}",
        "Analyze".blue().bold(),
        total_errors_display
    );
    Ok(true)
}

fn check_kernel_version(version: &str) -> Result<(), anyhow::Error> {
    let version: Vec<&str> = version.splitn(2, "-").collect();
    let version: Vec<&str> = version[0].split(".").collect();

    let min_version: Vec<&str> = MIN_KERNEL_VERSION.split(".").collect();

    for i in 0..3 {
        let ver_num: u32 = version[i].parse().unwrap();
        let min_ver_num: u32 = min_version[i].parse().unwrap();
        let incompatible = match i {
            0 => {
                if ver_num > min_ver_num {
                    break;
                }
                if ver_num == min_ver_num {
                    continue;
                }
                true
            }
            1 => {
                if ver_num > min_ver_num {
                    break;
                }
                if ver_num == min_ver_num {
                    continue;
                }
                true
            }
            2 => {
                if ver_num > min_ver_num {
                    break;
                }
                if ver_num == min_ver_num {
                    continue;
                }
                true
            }
            _ => unreachable!("Should not be reached!"),
        };

        if incompatible {
            println!(
                "{}: Kernel version {}\n",
                "Analyze".blue().bold(),
                "(not ok)".red().bold()
            );
            return Err(anyhow!(
                "Incompatible kernel version!\nMinimal compatible version: {}",
                MIN_KERNEL_VERSION
            ));
        }
    }
    println!(
        "{}: Kernel version {}\n",
        "Analyze".blue().bold(),
        "(ok)".green().bold()
    );
    Ok(())
}

fn check_kernel_version_remote(session: &mut Session) -> Result<(), anyhow::Error> {
    let mut channel = session.channel_session().unwrap();
    channel.exec("uname -r").unwrap();
    let mut output = String::new();
    channel.read_to_string(&mut output).unwrap();

    println!(
        "{}: Kernel version: {}",
        "Analyze".blue().bold(),
        &output.trim()
    );
    check_kernel_version(&output)?;
    channel.wait_close()?;
    Ok(())
}

async fn check_packages(options: Analyze, nodename: &str) -> Result<(), anyhow::Error> {
    let missing_pkgs: Arc<Mutex<Vec<&str>>> = Arc::new(Mutex::new(Vec::new()));

    match nodename {
        "ubuntu" => unsafe {
            let tasks = UBUNTU_PACKAGES.map(|mut pkg| {
                let missing_pkgs: Arc<Mutex<Vec<&str>>> = missing_pkgs.clone();
                if pkg == "linux-tools-generic" {
                    let output =
                        String::from_utf8(Command::new("uname").arg("-r").output().unwrap().stdout)
                            .unwrap();
                    pkg = ("linux-tools-".to_string() + output.leak()).leak().trim();
                }
                tokio::spawn(async move {
                    let output = String::from_utf8(
                        tokio::process::Command::new("sh")
                            .args(["-c", format!("apt -qq list {}", pkg).as_str()])
                            .output()
                            .await
                            .unwrap()
                            .stdout,
                    )
                    .expect("Failed to get package");

                    if !output.contains("[installed]") {
                        missing_pkgs.lock().unwrap().push(pkg);
                    } else {
                        println!(
                            "{}: Package \"{}\" is installed.",
                            "Analyze".blue().bold(),
                            pkg.bold()
                        );
                    }
                })
            });
            for t in tasks {
                t.await?
            }
        },
        "archlinux" => {
            let output = String::from_utf8(
                Command::new("sh")
                    .args([
                        "-c",
                        format!("pacman -Qqen | grep -wE '{}'", ARCH_PACKAGES.join("|")).as_str(),
                    ])
                    .output()
                    .unwrap()
                    .stdout,
            )?;

            for pkg in ARCH_PACKAGES {
                if !output.contains(pkg) {
                    missing_pkgs.lock().unwrap().push(pkg);
                } else {
                    println!(
                        "{}: Package \"{}\" is installed.",
                        "Analyze".blue().bold(),
                        pkg.bold()
                    );
                }
            }
        }
        _ => return Err(anyhow!("Unsupported OS")),
    }

    if !missing_pkgs.lock().unwrap().is_empty() {
        if options.noconfirm.is_none() {
            let mut action = String::new();
            println!(
                "{}: Missing packages:\n - {}",
                "Analyze".blue(),
                missing_pkgs.lock().unwrap().join("\n - ")
            );
            print!(
                "{}: Attempt to install missing packages? [Y/n] ",
                "Analyze".blue().bold()
            );
            io::stdout().flush()?;
            io::stdin().read_line(&mut action)?;

            let action = action.trim().to_lowercase();

            if action != "y" && action != "yes" && !action.is_empty() {
                println!(
                    "{}: Required packages {}\n",
                    "Analyze".blue().bold(),
                    "(not ok)".red().bold(),
                );
                unsafe {
                    let pkgs = MISSING_PACKAGES.get().as_mut().unwrap();
                    pkgs.lock()
                        .unwrap()
                        .append(&mut missing_pkgs.lock().unwrap().clone());
                }

                return Err(anyhow!(format!(
                    "Missing packages:\n - {}",
                    missing_pkgs.lock().unwrap().join("\n - ")
                )));
            }
        }

        println!(
            "{}: Installing missing packages:\n - {}\n",
            "Analyze".blue().bold(),
            missing_pkgs.lock().unwrap().join("\n - ")
        );
        match nodename {
            "ubuntu" => {
                println!("{}: please wait...", "analyze".blue().bold(),);
                Command::new("sh").args([
                    "-c",
                    format!(
                        "sudo apt install --assume-yes {}",
                        missing_pkgs.lock().unwrap().join(" ")
                    )
                    .as_str(),
                ]);
            }
            "archlinux" => {
                println!("{}: please wait...", "analyze".blue().bold(),);
                Command::new("sudo")
                    .arg("pacman")
                    .arg("--noconfirm")
                    .arg("-S")
                    .arg(missing_pkgs.lock().unwrap().join(" "))
                    .output()?;
            }
            _ => return Err(anyhow!("Unsupported OS")),
        }
        println!(
            "{}: Missing packages installed succefully.\n",
            "Analyze".blue().bold()
        );
    } else {
        println!(
            "{}: Required packages {}\n",
            "Analyze".blue().bold(),
            "(ok)".green().bold(),
        );
    }
    Ok(())
}

fn check_packages_remote(
    options: Analyze,
    session: &mut Session,
    password: &str,
) -> Result<(), anyhow::Error> {
    let mut channel = session.channel_session().unwrap();
    channel.exec("uname -n").unwrap();
    let mut output = String::new();
    channel.read_to_string(&mut output).unwrap();
    let mut missing_pkgs: Vec<&str> = Vec::new();

    let nodename = output.clone().trim().to_string();

    match nodename.as_str() {
        "ubuntu" => unsafe {
            for pkg in UBUNTU_PACKAGES {
                output = String::new();
                let mut channel = session.channel_session().unwrap();
                channel
                    .exec(format!("apt -qq list {}", pkg).as_str())
                    .unwrap();
                channel.read_to_string(&mut output).unwrap();

                if !output.contains("[installed]") {
                    missing_pkgs.push(pkg);
                } else {
                    println!(
                        "{}: Package \"{}\" is installed.",
                        "Analyze".blue().bold(),
                        pkg.bold()
                    );
                }

                channel.wait_close()?;
            }
        },
        "archlinux" => {
            let mut channel = session.channel_session().unwrap();
            channel
                .exec(format!("pacman -Qqen | grep {}", ARCH_PACKAGES.join(" ")).as_str())
                .unwrap();
            channel.read_to_string(&mut output).unwrap();
            channel.wait_close()?;

            for pkg in ARCH_PACKAGES {
                if !output.contains(pkg) {
                    missing_pkgs.push(pkg);
                } else {
                    println!(
                        "{}: Package \"{}\" is installed.",
                        "Analyze".blue().bold(),
                        pkg.bold()
                    );
                }
            }
        }
        _ => return Err(anyhow!("Unsupported OS: {}", output)),
    }

    if !missing_pkgs.is_empty() {
        if options.noconfirm.is_none() {
            let mut action = String::new();
            println!(
                "{}: Missing packages:\n - {}",
                "Analyze".blue(),
                missing_pkgs.join("\n - ")
            );
            print!(
                "{}: Attempt to install missing packages? [Y/n] ",
                "Analyze".blue().bold()
            );
            io::stdout().flush()?;
            io::stdin().read_line(&mut action)?;

            let action = action.trim().to_lowercase();

            if action != "y" && action != "yes" && !action.is_empty() {
                unsafe {
                    let pkgs = MISSING_PACKAGES.get().as_mut().unwrap();
                    pkgs.lock().unwrap().append(&mut missing_pkgs.clone());
                }
                return Err(anyhow!(format!(
                    "Missing packages:\n - {}",
                    missing_pkgs.join("\n - ")
                )));
            }
        }

        match nodename.as_str() {
            "ubuntu" => {
                output = String::new();
                channel = session.channel_session().unwrap();
                channel
                    .exec(
                        format!(
                            "echo {} | sudo -S apt install --assume-yes {}",
                            password,
                            missing_pkgs.join(" ")
                        )
                        .as_str(),
                    )
                    .unwrap();
                channel.read_to_string(&mut output).unwrap();

                channel.wait_close()?;
            }
            "archlinux" => {
                output = String::new();
                channel = session.channel_session().unwrap();
                channel
                    .exec(
                        format!(
                            "echo {} | sudo -S pacman --noconfirm -S {}",
                            password,
                            missing_pkgs.join(" ")
                        )
                        .as_str(),
                    )
                    .unwrap();
                channel.read_to_string(&mut output).unwrap();

                channel.wait_close()?;
            }
            _ => return Err(anyhow!("Unsupported OS: {}", output)),
        }
        println!(
            "{}: Missing packages installed succefully.",
            "Analyze".blue().bold()
        );
    } else {
        println!(
            "{}: Required packages {}\n",
            "Analyze".blue().bold(),
            "(ok)".green().bold(),
        );
    }
    channel.wait_close()?;
    Ok(())
}

fn check_bpf_enabled(flags: Vec<&str>) -> Result<(), anyhow::Error> {
    let mut missing_flags: Vec<&str> = Vec::new();

    for f in flags {
        if !f.trim().contains("is set to y") {
            missing_flags.push(f);
        }
    }

    if !missing_flags.is_empty() {
        return Err(anyhow!(
            "Missing kernel flags:\n - {}",
            missing_flags.join("\n - ")
        ));
    }

    println!(
        "{}: Required kernel flags {}\n",
        "Analyze".blue().bold(),
        "(ok)".green().bold()
    );

    Ok(())
}
fn check_bpf_enabled_remote(session: &mut Session, password: &str) -> Result<(), anyhow::Error> {
    let mut channel = session.channel_session().unwrap();
    channel
        .exec( format!(
            "echo {} | (sudo bpftool feature | rg -w 'CONFIG_BPF|CONFIG_BPF_SYSCALL|CONFIG_BPF_JIT|CONFIG_BPF_EVENTS')",
            password
        )
            .as_str(),
        )
        .unwrap();
    let mut output = String::new();
    channel.read_to_string(&mut output).unwrap();
    channel.wait_close()?;

    channel = session.channel_session().unwrap();
    channel
        .exec(
            format!(
                "echo {} | (sudo -S bpftool feature | rg -w 'CONFIG_HAVE_EBPF_JIT|CONFIG_HAVE_BPF_JIT')",
                password
            )
            .as_str(),
        )
        .unwrap();
    channel.read_to_string(&mut output).unwrap();
    channel.wait_close()?;

    let flags: Vec<&str> = output.trim().split("\n").collect();
    check_bpf_enabled(flags)?;

    Ok(())
}

fn check_net_iface(iface: &str, ifaces: Vec<&str>) -> Result<(), anyhow::Error> {
    if ifaces.contains(&iface) {
        println!(
            "{}: Network interface \"{}\" {}\n",
            "Analyze".blue().bold(),
            iface.bold(),
            "(ok)".green().bold(),
        );
    } else {
        println!(
            "{}: Network interface \"{}\" {}\n",
            "Analyze".blue().bold(),
            iface.bold(),
            "(not ok)".red().bold(),
        );
        return Err(anyhow!("Interface \"{}\" is not available", iface.bold()));
    }

    Ok(())
}

fn check_net_iface_remote(session: &mut Session, iface: &str) -> Result<(), anyhow::Error> {
    println!("{}: Checking network interfaces", "Analyze".blue().bold());
    let mut channel = session.channel_session().unwrap();
    channel
        .exec(
            "ip -o link show | awk -F': ' '{print $2}'"
                .to_string()
                .as_str(),
        )
        .unwrap();
    let mut output = String::new();
    channel.read_to_string(&mut output).unwrap();
    channel.wait_close()?;

    let ifaces: Vec<&str> = output.split("\n").collect();
    check_net_iface(iface, ifaces)?;

    Ok(())
}
