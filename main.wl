!@import blue;

!chan = std:sync:mpsc:new[];

!cl = std:mqtt:client:new chan "wlctrl" "127.0.0.1" 18854;
!_ = cl.subscribe "led/0" /$e { std:displayln "error subscribe!?" }[];

!bta = blue:new_adapter[];

#std:displayln ~ blue:list bta :s => 4;

!addr = $b"\x98\xD3q\xF6\x11\x0E";
!port = $n;# blue:spawn_port_for_address bta addr chan;
std:displayln ~ port;

!reconnect = {
    .port = blue:spawn_port_for_address bta addr chan;
};

reconnect[];

#!cmd = $b"#c22ffff c99ffff ceeffff L0009; %l03!";
!cmd = $b"#c22ffff L0009; +0000!";

!handle_frontend_command = {!(path, data) = @;
    match data
        $["one_color", x] => {
            !color = $\.x $p(1, -1);
            .color = std:v:hex2rgba_f color;
            .color = std:v:rgb2hsv color;
            .color =
                std:str:to_lowercase ~
                    std:bytes:to_hex ~
                        (byte (color.0 / 360.0) * 255.0)
                            (byte color.1 * 255.0)
                            (byte color.2 * 255.0);
            .color = "c" color;

            .cmd = std:str:to_bytes ~ "#" color " L0009; +0000!";
            std:displayln ">" cmd;
            on_error {||
                std:displayln "ER:" @;
                reconnect[];
            } ~ port.send cmd;
        };
};

!last_update_time = std:time:now[:ms] - 5000;

while $t {
    !now_ms = std:time:now :ms;
    if (now_ms - last_update_time) >= 5000 {
        std:displayln "SEND:" cmd;
        .last_update_time = now_ms;
        on_error {||
            std:displayln "ER:" @;
            reconnect[];
        } ~ port.send cmd;
    };

    !recv = $t;
    while recv {
        .recv = $f;

        match chan.try_recv[] $o(x) => {
            std:displayln "RECEIVED:" $\.x;
            match $\.x
                $p("led/0", x) => {
                    !pld = std:deser:json $\.x;
                    handle_frontend_command "led/0" pld;
                };
            .recv = $t;
        };
    };
    std:thread:sleep :ms => 100;
};

