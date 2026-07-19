use crate::bytecode::Module;
use crate::errors::RuntimeError;
use crate::interpreter::context::{Context, Data};
use crate::interpreter::native::NativeSignature;
use crate::native_abi::{NativeExtension, P7ExtensionInit};
use std::cell::RefCell;
use std::mem::ManuallyDrop;
use std::path::Path;
use std::rc::{Rc, Weak};

#[derive(Debug)]
pub enum CallOutcome {
    Returned(Option<Data>),
    Threw(Data),
    Trapped(RuntimeError),
}

pub struct Runtime {
    context: ManuallyDrop<Box<Context>>,
    released_roots: Rc<RefCell<Vec<usize>>>,
    native_extensions: Vec<NativeExtension>,
    shutdown_started: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            context: ManuallyDrop::new(Box::new(Context::new())),
            released_roots: Rc::new(RefCell::new(Vec::new())),
            native_extensions: Vec::new(),
            shutdown_started: false,
        }
    }

    pub fn load_module(&mut self, module: Module) {
        self.assert_running();
        self.context.load_module(module);
    }

    pub fn set_script_dir(&mut self, script_dir: Option<String>) {
        self.assert_running();
        self.context.set_script_dir(script_dir);
    }

    pub fn register_native_extension(
        &mut self,
        initializer: P7ExtensionInit,
    ) -> Result<(), RuntimeError> {
        self.ensure_running()?;
        crate::native_abi::register_initializer(&mut self.context, initializer)
    }

    pub fn load_native_extension(&mut self, path: &Path) -> Result<(), RuntimeError> {
        self.ensure_running()?;
        let extension = NativeExtension::load(&mut self.context, path)?;
        self.native_extensions.push(extension);
        Ok(())
    }

    pub fn register_native_function<F>(
        &mut self,
        name: impl Into<String>,
        signature: NativeSignature,
        function: F,
    ) where
        F: Fn(&mut Context, &[Data]) -> Result<Option<Data>, RuntimeError> + 'static,
    {
        self.assert_running();
        self.context
            .register_native_function(name, signature, function);
    }

    pub fn call(&mut self, name: &str, args: Vec<Data>) -> Result<CallOutcome, RuntimeError> {
        self.ensure_running()?;
        self.flush_released_roots();
        if !self.context.has_function(name) {
            return Err(RuntimeError::FunctionNotFound);
        }
        let expected_arity = self
            .context
            .function_arity("$root", name)
            .ok_or(RuntimeError::FunctionNotFound)?;
        if args.len() != expected_arity {
            return Err(RuntimeError::Other(format!(
                "Function '{}' expects {} argument(s), got {}",
                name,
                expected_arity,
                args.len()
            )));
        }
        let base_len = self.context.stack[0].stack.len();
        self.context.push_function(name, args);
        if let Err(error) = self.context.resume() {
            self.flush_released_roots();
            return Ok(CallOutcome::Trapped(error));
        }
        let output = if self.context.stack[0].stack.len() > base_len {
            self.context.stack[0].stack.pop()
        } else {
            None
        };
        let outcome = match output {
            Some(Data::Exception(value)) => CallOutcome::Threw(Data::Int(value)),
            value => CallOutcome::Returned(value),
        };
        self.flush_released_roots();
        Ok(outcome)
    }

    pub fn root(&mut self, value: Data) -> RootedValue {
        self.assert_running();
        self.flush_released_roots();
        let index = self.context.add_external_root(value);
        RootedValue {
            inner: Rc::new(RootInner {
                context_id: self.context.instance_id(),
                index,
                released_roots: Rc::downgrade(&self.released_roots),
            }),
        }
    }

    pub fn root_callback(&mut self, value: Data) -> Result<CallbackHandle, RuntimeError> {
        if !matches!(value, Data::Closure { .. }) {
            return Err(RuntimeError::Other(format!(
                "Expected closure for callback root, got {value:?}"
            )));
        }
        Ok(CallbackHandle {
            root: self.root(value),
        })
    }

    pub fn context(&self) -> &Context {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut Context {
        self.assert_running();
        self.flush_released_roots();
        &mut self.context
    }

    pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
        self.shutdown_started = true;
        self.flush_released_roots();
        while let Some(extension) = self.native_extensions.last_mut() {
            extension.shutdown(&mut self.context)?;
            self.native_extensions.pop();
        }
        self.flush_released_roots();
        Ok(())
    }

    fn flush_released_roots(&mut self) {
        for index in self.released_roots.borrow_mut().drain(..) {
            self.context.remove_external_root(index);
        }
    }

    fn ensure_running(&self) -> Result<(), RuntimeError> {
        if self.shutdown_started {
            Err(RuntimeError::Other(
                "Runtime shutdown has already started".to_string(),
            ))
        } else {
            Ok(())
        }
    }

    fn assert_running(&self) {
        assert!(
            !self.shutdown_started,
            "Runtime shutdown has already started"
        );
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        if self.shutdown().is_ok() {
            // SAFETY: Context is wrapped solely to preserve its host pointer
            // after a failed extension shutdown. Successful shutdown empties
            // the extension list before the context is destroyed.
            unsafe { ManuallyDrop::drop(&mut self.context) };
        }
    }
}

#[derive(Clone)]
pub struct RootedValue {
    inner: Rc<RootInner>,
}

struct RootInner {
    context_id: u64,
    index: usize,
    released_roots: Weak<RefCell<Vec<usize>>>,
}

impl Drop for RootInner {
    fn drop(&mut self) {
        if let Some(released_roots) = self.released_roots.upgrade() {
            released_roots.borrow_mut().push(self.index);
        }
    }
}

impl RootedValue {
    pub fn get(&self, context: &Context) -> Result<Data, RuntimeError> {
        if context.instance_id() != self.inner.context_id {
            return Err(RuntimeError::Other(
                "Rooted value belongs to a different runtime".to_string(),
            ));
        }
        context.external_root(self.inner.index).ok_or_else(|| {
            RuntimeError::Other("Rooted value has already been released".to_string())
        })
    }
}

#[derive(Clone)]
pub struct CallbackHandle {
    root: RootedValue,
}

impl CallbackHandle {
    pub fn invoke(
        &self,
        context: &mut Context,
        args: Vec<Data>,
    ) -> Result<CallOutcome, RuntimeError> {
        let closure = self.root.get(context)?;
        match context.call_closure(&closure, args) {
            Ok(Data::Exception(value)) => Ok(CallOutcome::Threw(Data::Int(value))),
            Ok(value) => Ok(CallOutcome::Returned(Some(value))),
            Err(error) => Ok(CallOutcome::Trapped(error)),
        }
    }
}
