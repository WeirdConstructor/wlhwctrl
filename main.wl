!@import blue;

!chan = std:sync:mpsc:new[];

!cl = std:mqtt:client:new chan "wlctrl" "127.0.0.1" 18854;
!_ = cl.subscribe "led/0" /$e { std:displayln "error subscribe!?" }[];

!bta = blue:new_adapter[];

#std:displayln ~ blue:list bta :s => 4;

!addr = $b"\x98\xD3q\xF6\x11\x0E";
!port = blue:spawn_port_for_address bta addr;
std:displayln ~ port;


while $t {
    std:displayln "SEND!";
    port.send $b"#c22ffff c99ffff ceeffff L0009; %l03!";
    match chan.try_recv[] $o(x) => {
        std:displayln "RECEIVED:" $\.x;
    };
    std:thread:sleep :s => 5;
};

