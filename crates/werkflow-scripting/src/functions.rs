pub(crate) mod rand {
    use std::iter;

    use crate::ScriptEngine;
    use crate::ScriptEnginePlugin;
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    use rhai::RegisterFn;

    pub struct Plugin;

    impl ScriptEnginePlugin for Plugin {
        fn init(&self, host: &mut ScriptEngine) {
            host.engine
                .register_fn("rand_float", rand_u64)
                .register_fn("rand_float", rand_f64)
                .register_fn("rand_bytes", rand_bytes)
                .register_fn("rand_string", rand_string);
        }
    }
    pub(crate) fn rand_u64() -> u64 {
        rand::thread_rng().gen()
    }
    pub(crate) fn rand_f64() -> f64 {
        rand::thread_rng().gen()
    }
    pub(crate) fn rand_bytes() -> [u8; 32] {
        rand::thread_rng().gen()
    }
    pub(crate) fn rand_string(num: u64) -> String {
        let mut rng = rand::thread_rng();
        let chars: String = iter::repeat(())
            .map(|()| rng.sample(Alphanumeric))
            .take(num as usize)
            .collect();

        chars
    }
}
