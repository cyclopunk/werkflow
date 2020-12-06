use std::{any::Any, collections::HashMap, time::Duration, pin::Pin};
use futures::{future::Either, Future, FutureExt, TryFutureExt, future::Select, pin_mut};
use rhai::Engine;
use tokio::{runtime::{Handle, Runtime}, sync::oneshot::Sender, task::JoinHandle};
use crate::Script;

pub struct ScriptScheduler {
    engine : Engine,
    runtime : Handle,
    cancel_tokens: HashMap<u128, Sender<()>>
} 

impl Default for ScriptScheduler {
    fn default() -> Self {
        ScriptScheduler {
            engine: Engine::new(),
            runtime: tokio::runtime::Handle::current(),
            cancel_tokens: HashMap::default(),
        }
    }
}

impl ScriptScheduler {

    pub fn schedule_with_engine(&mut self, engine: Engine, script : &Script, interval: Duration){
        let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
        
        let s = script.to_owned();
        
        let fut = async move {
            loop {
                engine.eval::<()>(&s.body).unwrap();
                tokio::time::delay_for(interval).await;
            }
        };

        let fut = Box::pin(fut);

        self.runtime.spawn(futures::future::select(fut, receiver.map_err(drop)));

        self.cancel_tokens.insert(script.identifier.id, sender);
    }

    pub fn cancel(&mut self, id: u128) {
        if let Some(token) = self.cancel_tokens.remove(&id) {
            token.send(()).expect("to send cancel token");          
        }    
    }

    pub fn cancel_all(&mut self){
        let keys : Vec<u128> = self.cancel_tokens.keys().cloned().collect();
        for n in keys {
            if let Some(token) = self.cancel_tokens.remove(&n) {
                token.send(()).expect("to send cancel token");          
            }   
        }
    }
}

#[tokio::test]
async fn test_schedule() {
    let mut s = ScriptScheduler::default();
    let body = r#"
print("Hi");
    "#;
    let script = Script::with_name("test",body);
    s.schedule_with_engine(Engine::new(), &script, Duration::from_secs(1));
    let mut n = 0;
    while n < 5 {
        tokio::time::delay_for(Duration::from_secs(1)).await;
        n+=1;
        
    }    
    s.cancel(script.identifier.id);
    println!("Canceled");
    tokio::time::delay_for(Duration::from_secs(5)).await;
}