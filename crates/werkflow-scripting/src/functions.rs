pub(crate) mod rand {
    use crate::ScriptHost;
use crate::ScriptHostPlugin;
use rhai::{Engine, RegisterFn};
    use rand::Rng;

    pub struct Plugin;

    impl ScriptHostPlugin for Plugin {
        fn init(&self, host : &mut ScriptHost){
            host.engine
                .register_fn("rand_float", u64)
                .register_fn("rand_float", f64)
                .register_fn("rand_bytes", bytes);
        }
    }
    pub(crate) fn u64() -> u64 {
        rand::thread_rng().gen()
    }
    pub(crate)  fn f64() -> f64 {
        rand::thread_rng().gen()
    }
    pub(crate) fn bytes() -> [u8; 32] {
        rand::thread_rng().gen()
    }
}