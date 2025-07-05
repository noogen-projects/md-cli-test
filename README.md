# md-cli-test

This is a library for integration testing of CLI applications in Rust using _markdown_ files as a source of test cases.

`Tester` automatically extracts and runs command-line examples from code blocks in `.md` specification files, verifying the correctness of CLI application's output. It is especially useful when following a _doctest_-like approach for CLI examples, helping keep your documentation and tests in sync.

## Features

- Parses H1 headers from markdown file (`# `) as test section titles
- Parses code blocks from markdown file (```` ```sh````, ```` ```shell````) as test cases
- Executes your CLI application and additional commands (`cd`, `ls`, `mkdir`, `rm`, `echo`, `cat`)
- Verifies expected output lines
- Supports Rust-style raw multi-line string arguments for commands

## Example

Markdown file `greeting.md`:

````md
# Greeting

```sh
$ my-cli greet Alice
Hello, Alice!
```
````

Test using this library:

```rust
use md_cli_test::Tester;

#[test]
fn greeting_test_cases() {
    Tester::new("tests/greeting.md").run().unwrap();
}
```

In more complex scenarios, you can also use aliase for binary, pass environment variables, and even use `cd`, `ls`, `mkdir`, `rm`, `echo`, `cat` commands.

For example, markdown file `new_project.md` for `todo-cli` application:

````md
# New project

## New project default

```sh
$ todo new "project A"
    Creating `project A` project
```

```sh
$ ls "./project A"
Project.toml
```

```sh
$ cat "./project A/Project.toml"
id = "project A"
name = "project A"
```

```sh
$ todo new "project A"
    Creating `project A` project
Error: destination `${current_dir_path}/project A` already exists
```
````

Integration test file `new_project.rs`:

```rust
use md_cli_test::Tester;

#[test]
fn new_project_test_cases() {
    Tester::new("tests/new_project.md")
        // Use `todo` alias instead of real package or binary name
        .with_cargo_bin_alias("todo")
        // Alias must be matched to a real binary name `todo-cli`
        // if it does not match the package name
        .with_cargo_bin_name("todo-cli")
        // Pass environment variable
        .with_env("TODO_CONFIG", "./todo.toml")
        .run()
        .unwrap();
}
```

## Usage

Add to your `Cargo.toml`:

```toml
[dev-dependencies]
md-cli-test = "0.1"
```

## Why use this?

- Keeps your documentation in sync with actual CLI behavior
- Avoids duplication between examples and tests
- Makes your documentation _executable_ and verifiable

## License

[MIT](LICENSE)
