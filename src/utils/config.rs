pub struct Config {
    pub server_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_port: 8080,
        }
    }
}

pub fn load_config() -> Config {
    // In a real-world scenario, this would load from a file or environment
    Config::default()
} 