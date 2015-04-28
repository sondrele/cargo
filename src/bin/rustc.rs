use std::env;

use std::path::Path;
use std::process::Output;
use std::sync::Arc;

use cargo::ops;
use cargo::ops::{ExecEngine, CommandPrototype, CompileOptions};
use cargo::core::{Package, Source};
use cargo::sources::PathSource;
use cargo::util::important_paths::find_root_manifest_for_cwd;
use cargo::util::{CliResult, CliError, Config};
use cargo::util::{CargoResult, ProcessError, ProcessBuilder};

#[derive(RustcDecodable, Debug)]
struct Options {
    arg_pkgid: Option<String>,
    arg_opts: Option<Vec<String>>,
    flag_jobs: Option<u32>,
    flag_features: Vec<String>,
    flag_no_default_features: bool,
    flag_profile: Option<String>,
    flag_target: Option<String>,
    flag_manifest_path: Option<String>,
    flag_verbose: bool,

    flag_release: bool,
}

pub const USAGE: &'static str = "
Compile a package and all of its dependencies

Usage:
    cargo rustc [options] [<pkgid>] [--] [<opts>...]

Options:
    -h, --help              Print this message
    -j N, --jobs N          The number of jobs to run in parallel
    --features FEATURES     Features to compile for the package
    --no-default-features   Do not compile default features for the package
    -p, --profile PROFILE   The profile to compile for
    --target TRIPLE         Target triple which compiles will be for
    --manifest-path PATH    Path to the manifest to fetch depednencies for
    -v, --verbose           Use verbose output
    --release               Build artifacts in release mode, with optimizations

The <pkgid> specified (defaults to the current package) will have all of its
dependencies compiled, and then the package itself will be compiled. This
command requires that a lockfile is available and dependencies have been
fetched.

All of the trailing arguments are passed through to the *final* rustc
invocation, not any of the dependencies.

Dependencies will not be recompiled if they do not need to be, but the package
specified will always be compiled. The compiler will receive a number of
arguments unconditionally such as --extern, -L, etc. Note that dependencies are
recompiled when the flags they're compiled with change, so it is not allowed to
manually compile a package's dependencies and then compile the package against
the artifacts just generated.
";

struct RustcEngine {
    args: Option<Vec<String>>,
    targets: Vec<String>,
}

impl ExecEngine for RustcEngine {
    fn exec(&self, command: CommandPrototype) -> Result<(), ProcessError> {
        append_rustc_opts(command, &self.args, &self.targets).exec()
    }

    fn exec_with_output(&self, command: CommandPrototype) -> Result<Output, ProcessError> {
        append_rustc_opts(command, &self.args, &self.targets).exec_with_output()
    }
}

fn append_rustc_opts(mut command: CommandPrototype, args: &Option<Vec<String>>, targets: &Vec<String>) -> ProcessBuilder {
    let name_matches = command.get_args().windows(2).find(|&args| {
        args[0].to_str() == Some("--crate-name") &&
        targets.iter().find(|&target| Some(target.as_ref()) == args[1].to_str()).is_some()
    }).is_some();

    if name_matches {
        if let &Some(ref args) = args {
            debug!("appending args to cmd; cmd={}; args={:?}", command, args);
            command.args(&args);
        }
    }
    command.into_process_builder()
}

fn get_package(root: &Path, config: &Config) -> CargoResult<Package> {
    let mut source = try!(PathSource::for_path(root.parent().unwrap(), &config));
    try!(source.update());
    source.root_package()
}

pub fn execute(options: Options, config: &Config) -> CliResult<Option<()>> {
    debug!("executing; cmd=cargo-rustc; args={:?}",
           env::args().collect::<Vec<_>>());
    config.shell().set_verbose(options.flag_verbose);

    let root = try!(find_root_manifest_for_cwd(options.flag_manifest_path));

    let package = try!(get_package(&root, &config));

    let pkgid = package.package_id();
    let spec = options.arg_pkgid.as_ref()
        .map(|s| if s == pkgid.name() {
            None
        } else {
            Some(&s[..])
        }).unwrap();

    let bins: Vec<String> = package.targets().iter()
        .filter(|t| t.is_bin())
        .map(|t| t.name().to_string())
        .collect();

    let engine = RustcEngine {
        args: options.arg_opts,
        targets: bins.clone()
    };

    let opts = CompileOptions {
        config: config,
        jobs: options.flag_jobs,
        target: options.flag_target.as_ref().map(|t| &t[..]),
        features: &options.flag_features,
        no_default_features: options.flag_no_default_features,
        spec: spec,
        exec_engine: Some(Arc::new(Box::new(engine))),
        mode: ops::CompileMode::Build,
        release: options.flag_release,
        filter: if bins.is_empty() {
            ops::CompileFilter::Everything
        } else {
            ops::CompileFilter::Only {
                lib: false, bins: &bins, examples: &[], benches: &[], tests: &[]
            }
        },
    };

    ops::compile(&root, &opts).map(|_| None).map_err(|err| {
        CliError::from_boxed(err, 101)
    })
}
