use super::syscalls;
use cranelift_codegen::ir::types;
use cranelift_codegen::{ir, isa};
use cranelift_entity::PrimaryMap;
use cranelift_wasm::DefinedFuncIndex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::rc::Rc;
use target_lexicon::HOST;
use wasi_common::WasiCtxBuilder;
use wasmtime_environ::{translate_signature, Export, Module};
use wasmtime_runtime::{Imports, InstanceHandle, InstantiationError, VMFunctionBody};

/// Return an instance implementing the "wasi" interface.
pub fn instantiate_wasi(
    prefix: &str,
    global_exports: Rc<RefCell<HashMap<String, Option<wasmtime_runtime::Export>>>>,
    preopened_dirs: &[(String, File)],
    argv: &[String],
    environ: &[(String, String)],
) -> Result<InstanceHandle, InstantiationError> {
    let pointer_type = types::Type::triple_pointer_type(&HOST);
    let mut module = Module::new();
    let mut finished_functions: PrimaryMap<DefinedFuncIndex, *const VMFunctionBody> =
        PrimaryMap::new();
    let call_conv = isa::CallConv::triple_default(&HOST);

    macro_rules! signature {
        ($name:ident) => {{
            let sig = module.signatures.push(translate_signature(
                ir::Signature {
                    params: syscalls::$name::params()
                        .into_iter()
                        .map(ir::AbiParam::new)
                        .collect(),
                    returns: syscalls::$name::results()
                        .into_iter()
                        .map(ir::AbiParam::new)
                        .collect(),
                    call_conv,
                },
                pointer_type,
            ));
            let func = module.functions.push(sig);
            module.exports.insert(
                prefix.to_owned() + stringify!($name),
                Export::Function(func),
            );
            finished_functions.push(syscalls::$name::SHIM as *const VMFunctionBody);
        }};
    }

    // unknown
    signature!(args_get);
    signature!(args_sizes_get);
    signature!(environ_get);
    signature!(environ_sizes_get);
    signature!(fd_allocate); // don't know what it does

    // need
    signature!(clock_res_get);
    signature!(clock_time_get);
    signature!(fd_close);
    signature!(fd_read);
    signature!(fd_renumber); // equivalent of dup2?
    signature!(fd_sync); // probably needed for flushing
    signature!(fd_write);
    signature!(poll_oneoff);
    signature!(proc_exit);
    signature!(random_get);
    signature!(sched_yield); // probably (related to frenetics?)
    signature!(sock_recv);
    signature!(sock_send);
    signature!(sock_shutdown);

    /// need equivalent of these but aren't standardized yet
    ///
    /// keeps must be able to establish connections from inside, as opposed to
    /// getting pre-established connections as filedescriptors
    /// * socket()
    /// * connect()
    /// * bind()
    /// * listen()
    /// * getsockopt()
    /// * setsockopt()
    /// * handshake() -- performs TLS handeshake, not POSIX

    // when we implement FS support
    signature!(fd_prestat_get); // used by the hello_world demo
    signature!(fd_prestat_dir_name);
    signature!(fd_datasync);
    signature!(fd_pread); // offset
    signature!(fd_pwrite); // offset
    signature!(fd_seek);
    signature!(fd_tell);
    signature!(fd_fdstat_get);
    signature!(fd_fdstat_set_flags);
    signature!(fd_fdstat_set_rights);
    signature!(fd_advise);
    signature!(path_create_directory);
    signature!(path_link);
    signature!(path_open);
    signature!(fd_readdir);
    signature!(path_readlink);
    signature!(path_rename);
    signature!(fd_filestat_get);
    signature!(fd_filestat_set_times);
    signature!(fd_filestat_set_size);
    signature!(path_filestat_get);
    signature!(path_filestat_set_times);
    signature!(path_symlink);
    signature!(path_unlink_file);
    signature!(path_remove_directory);

    // want to remove from WASI
    signature!(proc_raise); // related to signal handling

    let imports = Imports::none();
    let data_initializers = Vec::new();
    let signatures = PrimaryMap::new();

    let mut wasi_ctx_builder = WasiCtxBuilder::new()
        .and_then(|ctx| ctx.inherit_stdio())
        .and_then(|ctx| ctx.args(argv.iter()))
        .and_then(|ctx| ctx.envs(environ.iter()))
        .map_err(|err| {
            InstantiationError::Resource(format!("couldn't assemble WASI context object: {}", err))
        })?;

    for (dir, f) in preopened_dirs {
        wasi_ctx_builder = wasi_ctx_builder.preopened_dir(
            f.try_clone().map_err(|err| {
                InstantiationError::Resource(format!(
                    "couldn't clone an instance handle to pre-opened dir: {}",
                    err
                ))
            })?,
            dir,
        );
    }

    let wasi_ctx = wasi_ctx_builder.build().map_err(|err| {
        InstantiationError::Resource(format!("couldn't assemble WASI context object: {}", err))
    })?;

    InstanceHandle::new(
        Rc::new(module),
        global_exports,
        finished_functions.into_boxed_slice(),
        imports,
        &data_initializers,
        signatures.into_boxed_slice(),
        None,
        Box::new(wasi_ctx),
    )
}
