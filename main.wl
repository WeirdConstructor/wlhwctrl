!@import serial;
!@import blue;

!bta = blue:new_adapter[];

#std:displayln ~ blue:list bta :s => 4;

!addr = $b"\x98\xD3q\xF6\x11\x0E";
!port = blue:spawn_port_for_address bta addr;
std:displayln ~ port;

#port.send $b"#c22ffff c99ffff ceeffff L0009; %l03!";

while $t {
    std:thread:sleep :s => 4;
};

