use wlambda::*;
use wlambda::vval::VVal;
use wlambda::threads::AValChannel;
use std::rc::Rc;
use std::cell::RefCell;
use std::time::Duration;

use std::sync::{Arc, Mutex};


use futures::executor::block_on;
use futures::Future;
use futures::StreamExt;
use tokio::time::timeout;

use bluer::{
    AdapterEvent,
    agent::Agent,
    id::ServiceClass,
    rfcomm::{Profile, Socket, SocketAddr, ReqError, Stream, Role},
};

//type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone)]
struct BluetoothAdapter {
    rt:      tokio::runtime::Handle,
    session: bluer::Session,
    adapter: bluer::Adapter,
}

impl BluetoothAdapter {
    pub fn new(rt: tokio::runtime::Handle) -> Result<Self, bluer::Error> {
        let session = rt.block_on(async {
            bluer::Session::new().await
        })?;

        let adapter = rt.block_on(async {
            let adapters = session.adapter_names().await?;
            let adapter_name =
                adapters.get(0)
                    .ok_or_else(|| bluer::Error {
                        kind: bluer::ErrorKind::NotFound,
                        message: "No Adapters Found".to_string()
                    })?;
            session.adapter(adapter_name)
        })?;

        Ok(Self {
            rt,
            session,
            adapter,
        })
    }

    async fn discover_some_devices_impl(&self, devices: &mut Vec<(String, bluer::Address)>) -> Result<(), bluer::Error> {
        let mut disco_events = self.adapter.discover_devices().await.unwrap();

        while let Some(event) = disco_events.next().await {
            println!("EVENT: {:?}", event);
            match event {
                AdapterEvent::DeviceAdded(addr) => {
                    let cur_device = self.adapter.device(addr).unwrap();
                    let name = cur_device.name().await.unwrap();
                    println!("Device name: {:?}", name);
                    if let Some(name) = name {
                        devices.push((name.to_string(), addr));
                    }
                },
                _ => { },
            }
        }

        Ok(())
    }

    pub fn discover_some_devices(&mut self, dur: std::time::Duration)
        -> Result<Vec<(String, bluer::Address)>, bluer::Error>
    {
        let mut devices = vec![];

        let rt = self.rt.clone();

        rt.block_on(async {
            match timeout(dur, self.discover_some_devices_impl(&mut devices)).await {
                Ok(r) => r,
                Err(_) => Ok(()),
            }
        })?;

        Ok(devices)
    }

    pub fn spawn_client(&mut self,
        address: bluer::Address,
        recv_chan: Option<AValChannel>)
        -> Result<VVBluetoothSerialPort, bluer::Error>
    {
        let mut rt = self.rt.clone();
        VVBluetoothSerialPort::spawn(self, rt, address, recv_chan)
    }
}

#[derive(Debug, Clone)]
struct VVBluetoothAdapter {
    bta: Rc<RefCell<BluetoothAdapter>>,
}

impl VVBluetoothAdapter {
    pub fn new(bta: BluetoothAdapter) -> Self {
        Self { bta: Rc::new(RefCell::new(bta)) }
    }

    pub fn list(&self, dur: std::time::Duration) -> Result<VVal, bluer::Error> {
        let adrs = self.bta.borrow_mut().discover_some_devices(dur)?;
        let ret = VVal::vec();

        for (name, addr) in adrs {
            ret.push(VVal::map2(
                "name", VVal::new_str_mv(name),
                "addr", VVal::new_byt(addr.0.to_vec())));
        }

        Ok(ret)
    }

    pub fn spawn_client(&self,
        device: bluer::Address,
        recv_chan: Option<AValChannel>)
        -> Result<VVal, bluer::Error>
    {
        Ok(VVal::new_usr(
            self.bta.borrow_mut().spawn_client(device, recv_chan)?))
    }
}

impl VValUserData for VVBluetoothAdapter {
    fn s(&self) -> String {
        format!("$<BluetoothAdapter>")
    }
    fn as_any(&mut self) -> &mut dyn std::any::Any { self }
    fn clone_ud(&self) -> Box<dyn VValUserData> {
        Box::new(self.clone())
    }
}

#[derive(Debug)]
struct BluetoothSerialWriter {
    rt:     tokio::runtime::Handle,
    writer: bluer::rfcomm::stream::OwnedWriteHalf,
}

impl BluetoothSerialWriter {
    pub fn write(&mut self, buf: &[u8]) {
        use tokio::io::AsyncWriteExt;

        self.rt.block_on(async {
            self.writer.write_all(buf).await.unwrap();
        });
    }
}

#[derive(Debug, Clone)]
struct VVBluetoothSerialPort {
    port: Arc<Mutex<BluetoothSerialWriter>>,
}

async fn create_stream(
    bta:     &mut BluetoothAdapter,
    address: bluer::Address) -> Result<bluer::rfcomm::Stream, bluer::Error>
{
    let mut device = bta.adapter.device(address)?;
    println!("Device name: {:?}", device.name().await?);

//            let mut disco_events = adapter.discover_devices().await?;
    let mut dev_addr = address;

    let serial_uuid = ServiceClass::SerialPort.into();

    let agent = Agent::default();
    let _agent_hndl = bta.session.register_agent(agent).await?;

    let profile = Profile {
        uuid:                   serial_uuid,
        name:                   Some("rfcat client".to_string()),
        role:                   Some(Role::Client),
        require_authentication: Some(false),
        require_authorization:  Some(false),
        auto_connect:           Some(true),
        ..Default::default()
    };

    let mut hndl = bta.session.register_profile(profile).await?;

    let mut stream = loop {
        tokio::select! {
            res = async {
                let _ = device.connect().await;
                device.connect_profile(&serial_uuid).await
            } => {
                if let Err(err) = res {
                    println!("Connect profile failed: {:?}", err);
                }

                tokio::time::sleep(Duration::from_secs(3)).await;
            },
            req = hndl.next() => {
                let req = req.unwrap();

                println!("Connect req (wait for {}): {:?}", dev_addr, req);
                if req.device() == dev_addr {
                    break req.accept();
                    println!("ACCEPT!");
                } else {
                    req.reject(ReqError::Rejected);
                }
            }
        }
    };

    stream

//    println!("Connected Stream: {:?}", stream.peer_addr());
//    use tokio::io::AsyncWriteExt;
//    stream.write_all(b"#c22ffff c99ffff ceeffff L0009; %l03!").await.unwrap();
}

impl VVBluetoothSerialPort {
    pub fn spawn(
        bta:       &mut BluetoothAdapter,
        rt:        tokio::runtime::Handle,
        address:   bluer::Address,
        recv_chan: Option<AValChannel>) -> Result<Self, bluer::Error>
    {
        let stream : bluer::rfcomm::Stream =
            rt.block_on(create_stream(bta, address))?;

        let (mut reader, writer) = stream.into_split();

        let t_rt = rt.clone();

        std::thread::spawn(move || {
            use tokio::io::AsyncReadExt;

            t_rt.block_on(async {
                loop {
                    let mut buf = [0u8; 256];

                    match reader.read(&mut buf[..]).await {
                        Err(e) => {
                            // Send error!
                            println!("READ ERR: {}", e);
                        },
                        Ok(len) => {
                            if let Ok(s) = std::str::from_utf8(&buf[0..len]) {
                                println!("Read: '{:?}'", s);
                                if let Some(chan) = &recv_chan {
                                    chan.send(
                                        &VVal::vec3(
                                            VVal::new_sym("bt_data"),
                                            VVal::new_byt(address.to_vec()),
                                            VVal::new_str(s)));
                                }
                            }
                        },
                    }

                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });
        });

        Ok(VVBluetoothSerialPort {
            port: Arc::new(Mutex::new(BluetoothSerialWriter {
                rt,
                writer,
            })),
        })
    }
}

impl VValUserData for VVBluetoothSerialPort {
    fn s(&self) -> String {
        format!("$<BluetoothSerialPort>")
    }
    fn as_any(&mut self) -> &mut dyn std::any::Any { self }
    fn clone_ud(&self) -> Box<dyn VValUserData> {
        Box::new(self.clone())
    }
    fn as_thread_safe_usr(&mut self) -> Option<Box<dyn wlambda::threads::ThreadSafeUsr>> {
        Some(Box::new(self.clone()))
    }

    fn call_method(&self, key: &str, env: &mut Env) -> Result<VVal, StackAction> {
        let argv = env.argv_ref();
        match key {
            "send" => {
                if argv.len() != 1 {
                    return
                        Err(StackAction::panic_str(
                            "send method expects 1 argument".to_string(),
                            None,
                            env.argv()))
                }

                if let Ok(mut port) = self.port.lock() {
                    argv[0].with_bv_ref(|data|
                        port.write(data));
                }

                Ok(VVal::None)
            },
//            "subscribe" => {
//                if argv.len() != 1 {
//                    return
//                        Err(StackAction::panic_str(
//                            "subscribe method expects 1 argument".to_string(),
//                            None,
//                            env.argv()))
//                }
//
//                let ret = argv[0].with_s_ref(|s| self.subscribe(s));
//                match ret {
//                    Ok(_)  => Ok(VVal::Bol(true)),
//                    Err(e) => Ok(env.new_err(format!("subscribe error: {}", e)))
//                }
//            },
//            "publish" => {
//                if argv.len() != 2 {
//                    return
//                        Err(StackAction::panic_str(
//                            "publish method expects 2 argument".to_string(),
//                            None,
//                            env.argv()))
//                }
//
//                let ret =
//                    argv[0].with_s_ref(|topic|
//                        argv[1].with_bv_ref(|payload|
//                            self.publish(topic, payload)));
//                match ret {
//                    Ok(_)  => Ok(VVal::Bol(true)),
//                    Err(e) => Ok(env.new_err(format!("publish error: {}", e)))
//                }
//            },
            _ => {
                Err(StackAction::panic_str(
                    format!("unknown method called: {}", key),
                    None,
                    env.argv()))
            },
        }
    }

}

impl wlambda::threads::ThreadSafeUsr for VVBluetoothSerialPort {
    fn to_vval(&self) -> VVal {
        VVal::Usr(Box::new(self.clone()))
    }
}


//#[tokio::main(flavor = "current_thread")]
fn main() {
    let rt = Rc::new(RefCell::new(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()));

    let global_env = GlobalEnv::new_default();

    let argv = VVal::vec();
    for e in std::env::args() {
        argv.push(VVal::new_str_mv(e.to_string()));
    }
    global_env.borrow_mut().set_var("ARGV", &argv);

    let mut st = wlambda::SymbolTable::new();

//    st.set(
//        "hexo_consts_rs",
//        VVal::new_str(std::include_str!("ui/widgets/mod.rs")));

    st.fun(
        "list", move |_env: &mut Env, _argc: usize| {
            let ports = VVal::vec();

            for port in serialport::available_ports().unwrap() {
                ports.push(VVal::map2(
                    "name", VVal::new_str_mv(port.port_name.clone()),
                    "type", VVal::None));
            }

            Ok(ports)
        }, Some(0), Some(0), false);

    let mut bst = wlambda::SymbolTable::new();

/*

# API Idee:

!adapter = blue:adapter:new[];
!list = blue:list adapter :s => 9;

# - Check if new known device is available
# - Spawn Thread for that device with a broker client handle:

!recv_data_chan = std:sync:mpsc:new[];
!port = blue:serial_port adapter address recv_data_chan;

!client = mqtt client;

std:thread:spawn $code{
    loop {
    ... port.read_some[]
    ... port.read_timeout count duration
        if chan.try_recv[] {
            port.write $b"ieufwieufwehu";
        };
    }

} ${ port = port, chan = recv_data_chan };



*/
    bst.fun(
        "new_adapter", move |_env: &mut Env, _argc: usize| {
            let mut bta =
                BluetoothAdapter::new(
                    rt.borrow_mut().handle().clone()).unwrap();
            Ok(VVal::new_usr(VVBluetoothAdapter::new(bta)))
        }, Some(0), Some(0), false);

    bst.fun(
        "list", move |env: &mut Env, _argc: usize| {
//            let rt = tokio::runtime::Handle::current();
            let bta = env.arg(0);
            let dur = env.arg(1).to_duration()?;

            env.arg(0).with_usr_ref(|bta: &mut VVBluetoothAdapter| {
                match bta.list(dur) {
                    Err(e) =>
                        Ok(env.new_err(
                            format!("blue:list error: '{}'", e))),
                    Ok(v) => Ok(v)
                }
            }).unwrap_or_else(||
                Ok(env.new_err(
                    format!("blue:list expects a $<BluetoothAdapter> as first argument, got: '{}'",
                    env.arg(0).s()))))
        }, Some(2), Some(2), false);

    bst.fun(
        "spawn_port_for_address", move |env: &mut Env, _argc: usize| {
//            let rt = tokio::runtime::Handle::current();
            let bta  = env.arg(0);
            let addr = env.arg(1).as_bytes();
            if addr.len() != 6 {
                return Ok(env.new_err(
                    format!("blue:spawn_port_for_address address argument needs to be 6 bytes long, got: '{}'",
                    env.arg(1).s())));
            }

            let chan =
                if env.arg(2).is_some() {
                    let mut chan = env.arg(2);
                    let chan =
                        chan.with_usr_ref(|chan: &mut AValChannel| {
                            chan.fork_sender_direct()
                        });

                    if let Some(chan) = chan {
                       match chan {
                            Ok(chan) => Some(chan),
                            Err(err) => {
                                return
                                    Ok(VVal::err_msg(
                                        &format!("Failed to fork sender, \
                                                  can't get lock: {}", err)));
                            }
                       }
                    } else {
                        return
                            Ok(env.new_err(format!(
                                "bta:spawn_port_for_address: \
                                 channel not a std:sync:mpsc handle! {}",
                                env.arg(2).s())));
                    }
                } else {
                    None
                };

            let addr = bluer::Address::new(addr[0..6].try_into().unwrap());
            env.arg(0).with_usr_ref(|bta: &mut VVBluetoothAdapter| {
                match bta.spawn_client(addr, chan) {
                    Err(e) =>
                        Ok(env.new_err(
                            format!("blue:spawn_port_for_address error: '{}'", e))),
                    Ok(v) => Ok(v)
                }
            }).unwrap_or_else(||
                Ok(env.new_err(
                    format!("blue:spawn_port_for_address expects a $<BluetoothAdapter> as first argument, got: '{}'",
                    env.arg(0).s()))))
        }, Some(2), Some(3), false);

//            let mut bta = BluetoothAdapter::new(rt.borrow_mut().handle().clone()).unwrap();
//            bta.discover_some_devices(Duration::from_secs(9));
//            println!("STOP");
//            bta.discover_some_devices(Duration::from_secs(9));
//            println!("STOP");
//            bta.discover_some_devices(Duration::from_secs(9));


//                let sess = bluer::Session::new().await.unwrap();
//                // TODO: See: https://github.dev/bluez/bluer/tree/master/bluer
//                // TODO: And: https://docs.rs/bluer/0.13.3/bluer/struct.Session.html#method.new
//                let adapters = sess.adapter_names().await.unwrap();
//                println!("Adapters: {:?}", adapters);
//                let adapter = sess.adapter(adapters.get(0).unwrap()).unwrap();
//
//                let addrs = adapter.device_addresses().await.unwrap();
//                println!("Devices: {:#?}", addrs);
//                let mut device = adapter.device(addrs[0]).unwrap();
//                println!("Device name: {:?}", device.name().await.unwrap());
//
//                let mut disco_events = adapter.discover_devices().await.unwrap();
//                let mut dev_addr = addrs[0];
//
//                while let Some(event) = disco_events.next().await {
//                    println!("EVENT: {:?}", event);
//                    match event {
//                        AdapterEvent::DeviceAdded(addr) => {
//                            let cur_device = adapter.device(addr).unwrap();
//                            let name = cur_device.name().await.unwrap();
//                            println!("Device name: {:?}", name);
//                            if let Some(name) = name {
//                                if name == "HC-05" {
//                                    device = cur_device;
//                                    dev_addr = addr;
//                                    break;
//                                }
//                            }
//                        },
//                        _ => { },
//                    }
//                }
//
//                let serial_uuid = ServiceClass::SerialPort.into();
//
//                let agent = Agent::default();
//                let _agent_hndl = sess.register_agent(agent).await.unwrap();
//
//                let profile = Profile {
//                    uuid:                   serial_uuid,
//                    name:                   Some("rfcat client".to_string()),
//                    role:                   Some(Role::Client),
//                    require_authentication: Some(false),
//                    require_authorization:  Some(false),
//                    auto_connect:           Some(true),
//                    ..Default::default()
//                };
//
//                let mut hndl = sess.register_profile(profile).await.unwrap();
//
//                let mut stream = loop {
//                    tokio::select! {
//                        res = async {
//                            let _ = device.connect().await;
//                            device.connect_profile(&serial_uuid).await
//                        } => {
//                            if let Err(err) = res {
//                                println!("Connect profile failed: {:?}", err);
//                            }
//
//                            tokio::time::sleep(Duration::from_secs(3)).await;
//                        },
//                        req = hndl.next() => {
//                            let req = req.unwrap();
//
//                            println!("Connect req (wait for {}): {:?}", dev_addr, req);
//                            if req.device() == dev_addr {
//                                break req.accept().unwrap();
//                                println!("ACCEPT!");
//                            } else {
//                                req.reject(ReqError::Rejected);
//                            }
//                        }
//                    }
//                };
//
//                println!("Connected Stream: {:?}", stream.peer_addr());
//                use tokio::io::AsyncWriteExt;
//                stream.write_all(b"#c22ffff c99ffff ceeffff L0009; %l03!").await.unwrap();
//            });

    global_env.borrow_mut().set_module("serial", st);
    global_env.borrow_mut().set_module("blue", bst);

    let ctx = wlambda::EvalContext::new(global_env.clone());
    let ctx = Rc::new(RefCell::new(ctx));

    let mut ctx = ctx.borrow_mut();

    match ctx.eval_file(&std::env::args().nth(1).unwrap_or("main.wl".to_string())) {
        Ok(v) => {
            println!("Res: {}", v.s());
        },
        Err(e) => {
            println!("ERROR: {}", e);
        }
    }
}
