use clap::Parser;
use pnet::{
    datalink,
    packet::{
        ip::IpNextHeaderProtocols,
        ipv4::MutableIpv4Packet,
        ipv4::{self, Ipv4Packet},
        tcp::{self, MutableTcpPacket},
        Packet,
    },
    transport::{self, TransportChannelType},
};
use std::error::Error;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::time::Duration;

const BUF_SIZE: usize = 4096;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(required = true)]
    adders: Vec<SocketAddrV4>,

    #[arg(short, long, default_value = "roundrobin")]
    algorithem: String,

    #[arg(short, long, default_value_t = 8080)]
    port: u16,
}

#[derive(Debug)]
struct Connection {
    backend: Ipv4Addr,
    client: Ipv4Addr,
    port_mapper: (u16, u16),
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn handle_stream(stream: &mut TcpStream, dest: SocketAddrV4) -> Result<()> {
    let mut connection = TcpStream::connect(dest)?;
    let timeout_duration = Some(Duration::from_secs(5));
    let mut buf = [0; BUF_SIZE];

    stream.set_read_timeout(timeout_duration)?;
    stream.set_read_timeout(timeout_duration)?;
    connection.set_write_timeout(timeout_duration)?;
    connection.set_write_timeout(timeout_duration)?;

    while let Ok(_) = stream.read_exact(&mut buf) {
        connection.write_all(&buf)?;

        while let Ok(_) = connection.read_exact(&mut buf) {
            stream.write_all(&mut buf)?;
        }
    }

    Ok(())
}
fn get_local_ip() -> Vec<IpAddr> {
    datalink::interfaces()
        .iter()
        .find(|e| e.is_up() && !e.is_loopback() && !e.ips.is_empty())
        .expect("failed to find default interface")
        .ips
        .iter()
        .map(|x| x.ip())
        .collect()
}

fn main() -> Result<()> {
    let args = Args::parse();
    let listener = TcpListener::bind(format!("0.0.0.0:{}", args.port))?;
    let mut adders = args.adders.iter().cycle();
    let mut connections_pool: Vec<Connection> = Vec::new();

    println!("listening on :{}...", args.port);

    let (mut sender, mut recv) = transport::transport_channel(
        BUF_SIZE,
        TransportChannelType::Layer3(IpNextHeaderProtocols::Tcp),
    )?;

    let mut packets_iter = transport::ipv4_packet_iter(&mut recv);

    // TODO: support for ipv6
    // TODO: if backend is on the same server as lb this will create's a loop
    while let Ok((ipv4_packet, ip_adder)) = packets_iter.next() {
        let ip_adder = match ip_adder {
            IpAddr::V4(ip_adder) => ip_adder,
            IpAddr::V6(_) => todo!(),
        };
        let mut tcp_packet = MutableTcpPacket::owned(ipv4_packet.payload().to_vec()).unwrap();
        let mut ipv4_packet = MutableIpv4Packet::owned(ipv4_packet.packet().to_vec()).unwrap();

        if args.port == tcp_packet.get_destination() {
            if args
                .adders
                .iter()
                .find(|x| *x.ip() == ipv4_packet.get_source())
                .is_some()
            {
                println!("!found a packet with that port!");
                println!(
                    "{:?}:{:?}=>{:?}:{:?}",
                    ipv4_packet.get_source(),
                    tcp_packet.get_source(),
                    ipv4_packet.get_destination(),
                    tcp_packet.get_destination(),
                );
                // TODO: will forward any packets regardless of their ports
                let connection = connections_pool
                    .iter()
                    .find(|ip| IpAddr::V4(ip.backend) == ipv4_packet.get_source())
                    // .find(|ip| IpAddr::V4(ip.backend) == ipv4_packet.get_source() || ip.port_mapper == (tcp_packet.get_destination(), 8000))
                    .expect("couldn't find the port mapper on connections pool");

                ipv4_packet.set_source(Ipv4Addr::new(10, 5, 0, 6));
                ipv4_packet.set_destination(connection.client);
                tcp_packet.set_source(args.port);
                tcp_packet.set_destination(connection.port_mapper.0);
                tcp_packet.set_checksum(tcp::ipv4_checksum(
                    &tcp_packet.to_immutable(),
                    &Ipv4Addr::new(10, 5, 0, 6),
                    &connection.client,
                ));
                ipv4_packet.set_payload(tcp_packet.packet());
                ipv4_packet.set_checksum(ipv4::checksum(&ipv4_packet.to_immutable()));

                println!(
                    "send {:?} to client",
                    sender.send_to(ipv4_packet, IpAddr::V4(connection.client))?
                );

                continue;
            }

            let backend = adders.next().unwrap();

            let connection = Connection {
                backend: *backend.ip(),
                port_mapper: (tcp_packet.get_source(), backend.port()),
                client: ipv4_packet.get_source(),
            };

            println!("found a packet with that port!");
            println!(
                "{:?}:{:?}=>{:?}:{:?}",
                ipv4_packet.get_source(),
                tcp_packet.get_source(),
                ipv4_packet.get_destination(),
                tcp_packet.get_destination(),
            );
            // println!("{:?}", get_local_ip());

            ipv4_packet.set_source(Ipv4Addr::new(10, 5, 0, 6));
            ipv4_packet.set_destination(*backend.ip());
            tcp_packet.set_source(args.port);
            tcp_packet.set_destination(backend.port());
            tcp_packet.set_checksum(tcp::ipv4_checksum(
                &tcp_packet.to_immutable(),
                &Ipv4Addr::new(10, 5, 0, 6),
                &backend.ip(),
            ));
            ipv4_packet.set_payload(tcp_packet.packet());
            ipv4_packet.set_checksum(ipv4::checksum(&ipv4_packet.to_immutable()));

            println!(
                "{:?}:{:?}=>{:?}:{:?}",
                ipv4_packet.get_source(),
                tcp_packet.get_source(),
                ipv4_packet.get_destination(),
                tcp_packet.get_destination(),
            );

            println!("{:?}", backend.ip());
            println!(
                "send {:?} to backend",
                sender.send_to(ipv4_packet, IpAddr::V4(*backend.ip()))?
            );
            println!("{:?}", &connection);
            connections_pool.push(connection);
        }
    }
    Ok(())
}
