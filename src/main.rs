use wlambda;
use wlambda::vval::VVal;
use std::rc::Rc;
use std::cell::RefCell;


use futures::executor::block_on;

use bluer::{
    rfcomm::{Profile, Socket, SocketAddr, Stream},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

//#[tokio::main(flavor = "current_thread")]
fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();

    use wlambda::{GlobalEnv, Env};
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

    bst.fun(
        "list", move |_env: &mut Env, _argc: usize| {
//            let rt = tokio::runtime::Handle::current();
            let sess = rt.block_on(bluer::Session::new()).unwrap();
            // TODO: See: https://github.dev/bluez/bluer/tree/master/bluer
            // TODO: And: https://docs.rs/bluer/0.13.3/bluer/struct.Session.html#method.new
            println!("SESS: {:?}", sess);
//            let sock = Socket::new().unwrap();
////            let local_sa = 
//            let ports = VVal::vec();
//
//            for port in serialport::available_ports().unwrap() {
//                ports.push(VVal::map2(
//                    "name", VVal::new_str_mv(port.port_name.clone()),
//                    "type", VVal::None));
//            }
//
//            Ok(ports)
            Ok(VVal::None)
        }, Some(0), Some(0), false);

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
