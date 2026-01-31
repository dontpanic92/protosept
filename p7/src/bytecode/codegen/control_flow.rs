use super::Generator;

#[derive(Clone)]
pub(crate) struct LoopContext {
    pub(crate) break_patches: Vec<u32>, // Addresses of break jumps to patch
    pub(crate) continue_target: u32,    // Address to jump to for continue
}

impl Generator {
    pub(super) fn push_loop_context(&mut self, continue_target: u32) {
        self.loop_context_stack.push(LoopContext {
            break_patches: Vec::new(),
            continue_target,
        });
    }

    pub(super) fn pop_loop_context_and_patch_breaks(&mut self, loop_end: u32) {
        if let Some(ctx) = self.loop_context_stack.pop() {
            for break_addr in &ctx.break_patches {
                self.builder.patch_jump_address(*break_addr, loop_end);
            }
        }
    }

    pub(super) fn record_break(&mut self, break_jump_addr: u32) {
        if let Some(ctx) = self.loop_context_stack.last_mut() {
            ctx.break_patches.push(break_jump_addr);
        }
    }

    pub(super) fn current_continue_target(&self) -> Option<u32> {
        self.loop_context_stack
            .last()
            .map(|ctx| ctx.continue_target)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    struct DummyProvider;
    impl crate::ModuleProvider for DummyProvider {
        fn load_module(&self, _module_path: &str) -> Option<String> {
            None
        }
        
        fn clone_boxed(&self) -> Box<dyn crate::ModuleProvider> {
            Box::new(DummyProvider)
        }
    }

    fn mk_gen() -> Generator {
        Generator::new(Box::new(DummyProvider))
    }

    #[test]
    fn manages_loop_context_stack() {
        let mut g = mk_gen();
        g.push_loop_context(42);
        assert_eq!(g.loop_context_stack.len(), 1);
        g.record_break(10);
        g.pop_loop_context_and_patch_breaks(100);
        assert!(g.loop_context_stack.is_empty());
    }
}
