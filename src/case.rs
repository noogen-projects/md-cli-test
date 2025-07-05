use std::ffi::OsString;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::{env, fs, io, mem};

use assert_cmd::Command;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

use super::cmd::{Cmd, CmdResponse};
use crate::error::{self, TestError};

pub struct TestSection {
    pub title: String,
    pub cases: Vec<TestCase>,
}

#[derive(Debug, Default)]
pub struct TestCase {
    pub commands: Vec<String>,
    pub cargo_bin_alias: String,
    pub cargo_bin_name: Option<String>,
    pub test_dir: Option<PathBuf>,
    pub output: ExpectedOutput,
    pub envs: Vec<(OsString, OsString)>,
}

#[derive(Debug, Default)]
pub struct ExpectedOutput {
    pub text: String,
    pub source_path: Option<PathBuf>,
    pub source_line: Option<usize>,
}

enum Multiline {
    ToEndString(&'static str, String),
    WithLinesHasEnd(&'static str, String),
}

impl Deref for Multiline {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::ToEndString(_, string) => string,
            Self::WithLinesHasEnd(_, string) => string,
        }
    }
}

impl DerefMut for Multiline {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::ToEndString(_, string) => string,
            Self::WithLinesHasEnd(_, string) => string,
        }
    }
}

impl From<Multiline> for String {
    fn from(value: Multiline) -> Self {
        match value {
            Multiline::ToEndString(_, string) => string,
            Multiline::WithLinesHasEnd(_, string) => string,
        }
    }
}

impl TestCase {
    pub fn parse(source: impl AsRef<str>, source_path: Option<PathBuf>, source_line: Option<usize>) -> Self {
        let mut commands = Vec::new();
        let mut expected_output = String::new();
        let mut multiline_command: Option<Multiline> = None;

        // Split into commands and expected output
        for mut line in source.as_ref().lines() {
            if let Some(mut command) = multiline_command.take() {
                command.push('\n');

                let is_last_line = match &command {
                    Multiline::ToEndString(end, _) => line.contains(*end),
                    Multiline::WithLinesHasEnd(end, _) => {
                        if line.ends_with(*end) {
                            if line.len() > 1 {
                                line = &line[..line.len() - 1];
                            } else {
                                line = "";
                            }
                            false
                        } else {
                            true
                        }
                    },
                };

                command.push_str(line);
                if is_last_line {
                    commands.push(command.into());
                } else {
                    multiline_command = Some(command);
                }
                continue;
            }

            if line.starts_with("$") {
                let mut line = line.trim_start_matches('$').trim_start().to_string();

                let open_string_idx = line.rfind("#\"");
                let close_string_idx = line.rfind("\"#");
                if open_string_idx.is_some() && open_string_idx.map(|idx| idx + 1) >= close_string_idx {
                    multiline_command = Some(Multiline::ToEndString("\"#", line));
                } else {
                    let mark_string_count = line.matches("\"").count();
                    if mark_string_count % 2 == 1 {
                        multiline_command = Some(Multiline::ToEndString("\"", line));
                    } else if line.ends_with('\\') {
                        line.pop();
                        multiline_command = Some(Multiline::WithLinesHasEnd("\\", line));
                    } else {
                        commands.push(line);
                    }
                }
            } else if !commands.is_empty() {
                expected_output.push_str(line);
                expected_output.push('\n');
            }
        }

        if let Some(command) = multiline_command {
            commands.push(command.into());
        }

        // Remove trailing newline
        if !source.as_ref().ends_with('\n') && expected_output.ends_with('\n') {
            expected_output.pop();
        }

        Self {
            commands,
            cargo_bin_alias: String::new(),
            cargo_bin_name: None,
            test_dir: None,
            output: ExpectedOutput {
                text: expected_output,
                source_path,
                source_line,
            },
            envs: Vec::new(),
        }
    }

    pub fn with_cargo_bin_alias(mut self, alias: impl Into<String>, cargo_bin_name: Option<impl Into<String>>) -> Self {
        self.set_cargo_bin_alias(alias, cargo_bin_name);
        self
    }

    pub fn set_cargo_bin_alias(&mut self, alias: impl Into<String>, cargo_bin_name: Option<impl Into<String>>) {
        self.cargo_bin_alias = alias.into();
        self.cargo_bin_name = cargo_bin_name.map(Into::into);
    }

    pub fn with_test_dir(mut self, test_dir: impl Into<PathBuf>) -> Self {
        self.test_dir = Some(test_dir.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<OsString>, val: impl Into<OsString>) -> Self {
        self.envs.push((key.into(), val.into()));
        self
    }

    pub fn with_envs(mut self, vars: impl IntoIterator<Item = (impl Into<OsString>, impl Into<OsString>)>) -> Self {
        self.push_envs(vars);
        self
    }

    pub fn push_envs(&mut self, vars: impl IntoIterator<Item = (impl Into<OsString>, impl Into<OsString>)>) {
        for (key, val) in vars {
            self.envs.push((key.into(), val.into()));
        }
    }

    pub fn run(&self) -> error::Result<()> {
        let mut root_dir = self.test_dir.clone().unwrap_or_default();
        if !root_dir.exists() {
            return Err(TestError::Failed(format!(
                "Root directory `{}` does not exist",
                root_dir.display()
            )));
        }

        for command in &self.commands {
            match Cmd::parse(&root_dir, command) {
                Ok(cmd) => match cmd.run()? {
                    CmdResponse::Success => (),
                    CmdResponse::ChangeDirTo(path) => root_dir = path,
                    CmdResponse::Output(output) => self.assert_command_output(&root_dir, command, output),
                },
                Err(parts) => {
                    if let [name, args @ ..] = &parts[..] {
                        let mut cmd = if *name == self.cargo_bin_alias {
                            let bin_name = if let Some(bin_name) = &self.cargo_bin_name {
                                bin_name.clone()
                            } else {
                                env::var("CARGO_PKG_NAME")?
                            };

                            Command::cargo_bin(bin_name)?
                        } else {
                            Command::cargo_bin(name)?
                        };

                        let cmd_assert = cmd
                            .envs(self.envs.iter().map(|(key, val)| (key, val)))
                            .args(args)
                            .current_dir(&root_dir)
                            .assert();

                        let stdout = separate_logs(&String::from_utf8_lossy(&cmd_assert.get_output().stdout));
                        let stderr = separate_logs(&String::from_utf8_lossy(&cmd_assert.get_output().stderr));
                        let full_output = format!("{stdout}{stderr}");

                        self.assert_command_output(&root_dir, command, full_output);
                    } else {
                        return Err(TestError::Failed(format!("Invalid command `{command}`")));
                    }
                },
            }
        }

        Ok(())
    }

    pub fn assert_command_output(&self, root_dir: impl AsRef<Path>, command: impl AsRef<str>, output: impl AsRef<str>) {
        let root_dir = root_dir.as_ref();
        let command = command.as_ref();
        let output = output.as_ref();

        let expected_output = self
            .output
            .text
            .replace("${current_dir_path}", &root_dir.to_string_lossy());

        let source_path = self
            .output
            .source_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let source_line = self.output.source_line.unwrap_or_default();

        // On macOS, temporary directories may appear with a `/private` prefix,
        // e.g., `/private/var/folders/...`, which causes mismatch with expected output
        // defined as `/var/folders/...`. To ensure cross-platform consistency,
        // we normalize such paths in test output comparison.
        let normalized_output = output.replace("/private/var/", "/var/");

        assert_eq!(
            normalized_output, expected_output,
            "Command `{command}` in source {source_path}:{source_line}"
        );
    }
}

pub fn parse_markdown_tests(
    md_file_path: impl AsRef<Path>,
    cargo_bin_alias: Option<String>,
    cargo_bin_name: Option<String>,
    vars: Option<impl IntoIterator<Item = (impl Into<OsString>, impl Into<OsString>)> + Clone>,
) -> io::Result<Vec<TestSection>> {
    let md_file_path = md_file_path.as_ref();
    let content = fs::read_to_string(md_file_path)?;
    let parser = Parser::new(&content);

    let mut sections = Vec::new();
    let mut cases = Vec::new();
    let mut test_case = None;
    let mut test_case_start_line = None;
    let mut section_title = String::new();
    let mut in_test_case_code_block = false;
    let mut in_section_heading = false;

    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang)))
                if lang.as_ref() == "sh" || lang.as_ref() == "shell" =>
            {
                in_test_case_code_block = true;
                test_case_start_line = Some(content.split_at(range.start).0.lines().count() + 1);
            },
            Event::Text(text) if in_test_case_code_block => {
                let mut new_test_case = TestCase::parse(text, Some(md_file_path.into()), test_case_start_line);
                if let Some(alias) = cargo_bin_alias.clone() {
                    new_test_case.set_cargo_bin_alias(alias, cargo_bin_name.clone());
                }
                if let Some(vars) = vars.clone() {
                    new_test_case.push_envs(vars);
                }

                test_case = Some(new_test_case);
            },
            Event::End(TagEnd::CodeBlock) if in_test_case_code_block => {
                if let Some(test) = test_case.take() {
                    cases.push(test);
                }
                in_test_case_code_block = false;
            },
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) => {
                if !cases.is_empty() {
                    sections.push(TestSection {
                        title: mem::take(&mut section_title),
                        cases,
                    });
                    cases = Vec::new();
                }
                in_section_heading = true;
            },
            Event::Text(text) if in_section_heading => {
                section_title = text.to_string();
            },
            Event::End(TagEnd::Heading(HeadingLevel::H1)) if in_section_heading => {
                in_section_heading = false;
            },
            _ => {},
        }
    }

    if !cases.is_empty() {
        sections.push(TestSection {
            title: section_title,
            cases,
        });
    }

    Ok(sections)
}

fn separate_logs(source: &str) -> String {
    let mut outputs = source
        .lines()
        .filter(|line| {
            if line.trim().starts_with("[log]") {
                log::debug!("{line}");
                false
            } else {
                true
            }
        })
        .collect::<Vec<_>>();

    if source.ends_with('\n') {
        outputs.push("");
    }

    outputs.join("\n")
}

#[cfg(test)]
mod tests {
    use super::TestCase;

    #[test]
    fn parse_test_case() {
        let test = TestCase::parse(
            r#"
$ todo new "test A"
    Creating `test A` project
"#,
            None,
            None,
        );

        assert_eq!(test.commands.len(), 1);
        assert_eq!(test.commands[0], "todo new \"test A\"");
        assert_eq!(test.output.text, "    Creating `test A` project\n");

        let test = TestCase::parse(
            r#"
# Some comment
$ todo new "test A"
    Creating `test A` project"#,
            None,
            None,
        );

        assert_eq!(test.commands.len(), 1);
        assert_eq!(test.commands[0], "todo new \"test A\"");
        assert_eq!(test.output.text, "    Creating `test A` project");

        let test = TestCase::parse(
            r#"
# Some comment

$ mkdir "test A"
$ todo new "test A"
    Creating `test A` project
Error: destination `~/test A` already exists
"#,
            None,
            None,
        );

        assert_eq!(test.commands.len(), 2);
        assert_eq!(test.commands[0], "mkdir \"test A\"");
        assert_eq!(test.commands[1], "todo new \"test A\"");
        assert_eq!(
            test.output.text,
            "    Creating `test A` project\nError: destination `~/test A` already exists\n"
        );
    }
}
