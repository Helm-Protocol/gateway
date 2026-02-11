//! Helm Protocol CLI banner and theme.
//!
//! Theme: Bright khaki / forest green — symbolizing freedom and peace.
//! Every agent is a node. Every node is sovereign.

#![allow(dead_code)]

/// ANSI color codes for the Helm theme.
pub mod colors {
    /// Bright khaki (forest/freedom theme) — RGB(189, 183, 107) / ANSI 256-color
    pub const KHAKI: &str = "\x1b[38;2;189;183;107m";
    /// Warm gold accent
    pub const GOLD: &str = "\x1b[38;2;218;165;32m";
    /// Forest green
    pub const FOREST: &str = "\x1b[38;2;85;107;47m";
    /// Soft sage
    pub const SAGE: &str = "\x1b[38;2;143;151;121m";
    /// Light cream text
    pub const CREAM: &str = "\x1b[38;2;245;245;220m";
    /// Dim text
    pub const DIM: &str = "\x1b[2m";
    /// Bold text
    pub const BOLD: &str = "\x1b[1m";
    /// Reset all formatting
    pub const RESET: &str = "\x1b[0m";
}

/// Print the main Helm banner.
pub fn print_banner() {
    use colors::*;

    println!(r#"
{KHAKI}{BOLD}
    ██╗  ██╗███████╗██╗     ███╗   ███╗
    ██║  ██║██╔════╝██║     ████╗ ████║
    ███████║█████╗  ██║     ██╔████╔██║
    ██╔══██║██╔══╝  ██║     ██║╚██╔╝██║
    ██║  ██║███████╗███████╗██║ ╚═╝ ██║
    ╚═╝  ╚═╝╚══════╝╚══════╝╚═╝     ╚═╝{RESET}
{FOREST}    ─────────────────────────────────────{RESET}
{SAGE}     The Sovereign Agent Protocol{RESET}
{DIM}{CREAM}     Freedom · Peace · Autonomy{RESET}
"#);
}

/// Print a compact startup banner with node info.
pub fn print_startup(version: &str, node_name: &str, peer_id: &str) {
    use colors::*;

    println!("{FOREST}╭───────────────────────────────────────────╮{RESET}");
    println!("{FOREST}│{RESET} {KHAKI}{BOLD}Helm Protocol{RESET} {SAGE}v{version}{RESET}");
    println!("{FOREST}│{RESET} {CREAM}Node:{RESET}    {GOLD}{node_name}{RESET}");
    println!("{FOREST}│{RESET} {CREAM}Peer ID:{RESET} {DIM}{}{RESET}", &peer_id[..std::cmp::min(peer_id.len(), 16)]);
    println!("{FOREST}│{RESET}");
    println!("{FOREST}│{RESET} {SAGE}Every agent is a node.{RESET}");
    println!("{FOREST}│{RESET} {SAGE}Every node is sovereign.{RESET}");
    println!("{FOREST}╰───────────────────────────────────────────╯{RESET}");
}

/// Print module status during initialization.
pub fn print_module_status(module: &str, status: &str, ok: bool) {
    use colors::*;

    let icon = if ok {
        format!("{FOREST}✓{RESET}")
    } else {
        format!("\x1b[31m✗{RESET}")
    };

    println!("  {icon} {KHAKI}{module}{RESET} {DIM}{SAGE}— {status}{RESET}");
}

/// Print a horizontal divider.
pub fn print_divider() {
    use colors::*;
    println!("{FOREST}  ─────────────────────────────────────────{RESET}");
}

/// Print a section header.
pub fn print_section(title: &str) {
    use colors::*;
    println!();
    println!("  {GOLD}{BOLD}{title}{RESET}");
    print_divider();
}

/// Print a key-value info line.
pub fn print_info(key: &str, value: &str) {
    use colors::*;
    println!("  {SAGE}{key}:{RESET} {CREAM}{value}{RESET}");
}

/// Print engine stats.
pub fn print_engine_stats(
    pool_total: usize,
    pool_active: usize,
    sequences: usize,
) {
    use colors::*;

    println!("  {KHAKI}Engine{RESET}");
    println!("    {SAGE}Block Pool:{RESET}  {CREAM}{pool_active}/{pool_total} active{RESET}");
    println!("    {SAGE}Sequences:{RESET}   {CREAM}{sequences}{RESET}");
}

/// Print store stats.
pub fn print_store_stats(backend: &str, keys: usize) {
    use colors::*;

    println!("  {KHAKI}Store{RESET}");
    println!("    {SAGE}Backend:{RESET}     {CREAM}{backend}{RESET}");
    println!("    {SAGE}Keys:{RESET}        {CREAM}{keys}{RESET}");
}

/// Print the version string.
pub fn version_string() -> String {
    format!("Helm Protocol v{} (helm-node)", env!("CARGO_PKG_VERSION"))
}

/// Print the farewell message.
pub fn print_farewell() {
    use colors::*;
    println!();
    println!("{FOREST}  ╭─────────────────────────────────────╮{RESET}");
    println!("{FOREST}  │{RESET} {SAGE}Node shutting down gracefully.{RESET}     {FOREST}│{RESET}");
    println!("{FOREST}  │{RESET} {DIM}{CREAM}The forest remembers every path.{RESET}  {FOREST}│{RESET}");
    println!("{FOREST}  ╰─────────────────────────────────────╯{RESET}");
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_does_not_panic() {
        print_banner();
    }

    #[test]
    fn startup_does_not_panic() {
        print_startup("0.1.0", "test-node", "12D3KooWABCDEF1234567890");
    }

    #[test]
    fn module_status_does_not_panic() {
        print_module_status("helm-core", "runtime initialized", true);
        print_module_status("helm-net", "transport failed", false);
    }

    #[test]
    fn version_string_format() {
        let v = version_string();
        assert!(v.contains("Helm Protocol"));
        assert!(v.contains("helm-node"));
    }

    #[test]
    fn farewell_does_not_panic() {
        print_farewell();
    }

    #[test]
    fn all_colors_defined() {
        // Verify ANSI escape sequences are well-formed
        assert!(colors::KHAKI.starts_with("\x1b["));
        assert!(colors::GOLD.starts_with("\x1b["));
        assert!(colors::FOREST.starts_with("\x1b["));
        assert!(colors::SAGE.starts_with("\x1b["));
        assert!(colors::CREAM.starts_with("\x1b["));
        assert!(colors::RESET.starts_with("\x1b["));
    }
}
