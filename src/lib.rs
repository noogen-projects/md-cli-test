use std::ffi::OsString;
use std::path::PathBuf;

use temp_testdir::TempDir;

pub mod case;
pub mod cmd;
pub mod error;

#[derive(Debug, Clone)]
pub struct Tester {
    pub md_file_path: PathBuf,
    pub cargo_bin_alias: Option<String>,
    pub cargo_bin_name: Option<String>,
    pub envs: Vec<(OsString, OsString)>,
}

impl Tester {
    pub fn new(md_file_path: impl Into<PathBuf>) -> Self {
        Self {
            md_file_path: md_file_path.into(),
            cargo_bin_alias: None,
            cargo_bin_name: None,
            envs: Vec::new(),
        }
    }

    pub fn with_cargo_bin_alias(mut self, alias: impl Into<String>) -> Self {
        self.cargo_bin_alias = Some(alias.into());
        self
    }

    pub fn with_cargo_bin_name(mut self, cargo_bin_name: impl Into<String>) -> Self {
        self.cargo_bin_name = Some(cargo_bin_name.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<OsString>, val: impl Into<OsString>) -> Self {
        self.envs.push((key.into(), val.into()));
        self
    }

    pub fn with_envs(mut self, vars: impl IntoIterator<Item = (impl Into<OsString>, impl Into<OsString>)>) -> Self {
        for (key, val) in vars {
            self.envs.push((key.into(), val.into()));
        }
        self
    }

    pub fn run(self) -> error::Result<()> {
        let sections = case::parse_markdown_tests(
            self.md_file_path,
            self.cargo_bin_alias,
            self.cargo_bin_name,
            Some(self.envs),
        )?;

        for section in sections {
            let test_dir = TempDir::default();
            let mut completed_tests = Vec::new();

            log::debug!("\n# {}", section.title);

            for test_case in section.cases {
                let test_case = test_case.with_test_dir(test_dir.as_os_str());

                log::debug!("Testing: {:?}", test_case.commands);
                test_case.run()?;
                completed_tests.push(test_case);
            }

            // Destroy completed test cases
            drop(completed_tests);
        }
        Ok(())
    }
}
