use std::{sync::mpsc, thread};
pub struct Relay {
    pub endpoint: String,
    stop: tokio::sync::watch::Sender<bool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Relay {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let (stop, receiver) = tokio::sync::watch::channel(false);
        let worker = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                tx.send(format!("http://{}", listener.local_addr().unwrap()))
                    .unwrap();
                tide_jam_lab::relay::serve(listener, receiver)
                    .await
                    .unwrap();
            });
        });
        Self {
            endpoint: rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
