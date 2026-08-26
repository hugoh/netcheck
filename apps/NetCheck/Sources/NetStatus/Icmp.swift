import Darwin
import Foundation

extension Probe {
    private static let pingTimeout: TimeInterval = 3
    private static let pingPayload: [UInt8] = [0, 1, 2, 3, 4, 5, 6, 7]

    /// Sends one ICMP echo request to `target` (which must be a literal
    /// IPv4 or IPv6 address) over an unprivileged `SOCK_DGRAM` ICMP socket,
    /// bounded to 3s. An unparsable target is
    /// reported unreachable.
    public static func ping(_ target: String) async -> PingResult {
        await ping(target, echo: icmpEcho)
    }

    static func ping(
        _ target: String,
        echo: @Sendable @escaping (String) -> TimeInterval?
    ) async -> PingResult {
        await blocking {
            guard let rtt = echo(target) else {
                return PingResult(target: target, reachable: false, rttMs: nil)
            }
            return PingResult(target: target, reachable: true, rttMs: rtt * 1000)
        }
    }

    private static func icmpEcho(_ target: String) -> TimeInterval? {
        var v4 = in_addr()
        var v6 = in6_addr()
        let isV6: Bool
        if inet_pton(AF_INET, target, &v4) == 1 {
            isV6 = false
        } else if inet_pton(AF_INET6, target, &v6) == 1 {
            isV6 = true
        } else {
            return nil
        }

        let proto = isV6 ? IPPROTO_ICMPV6 : IPPROTO_ICMP
        let family = isV6 ? AF_INET6 : AF_INET
        let fd = socket(family, SOCK_DGRAM, proto)
        guard fd >= 0 else { return nil }
        defer { close(fd) }

        var timeout = timeval(
            tv_sec: Int(pingTimeout), tv_usec: Int32((pingTimeout - Double(Int(pingTimeout))) * 1e6)
        )
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))

        let sequence = UInt16.random(in: 0 ... UInt16.max)
        let echoType: UInt8 = isV6 ? 128 : 8
        let packet = icmpPacket(type: echoType, sequence: sequence, computeChecksum: !isV6)

        let start = Date()
        let sent = isV6
            ? sendEcho(fd: fd, packet: packet, to: &v6)
            : sendEcho(fd: fd, packet: packet, to: &v4)
        guard sent == packet.count else { return nil }

        let deadline = start.addingTimeInterval(pingTimeout)
        guard awaitEchoReply(fd: fd, sequence: sequence, isV6: isV6, deadline: deadline) else {
            return nil
        }
        return -start.timeIntervalSinceNow
    }

    private static func sendEcho(fd: Int32, packet: [UInt8], to addr6: inout in6_addr) -> Int {
        packet.withUnsafeBytes { buffer in
            withUnsafePointer(to: &addr6) { addr in
                var sa = sockaddr_in6()
                sa.sin6_family = sa_family_t(AF_INET6)
                sa.sin6_addr = addr.pointee
                return withUnsafePointer(to: &sa) { saPointer in
                    saPointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { rebound in
                        sendto(fd, buffer.baseAddress, buffer.count, 0, rebound,
                               socklen_t(MemoryLayout<sockaddr_in6>.size))
                    }
                }
            }
        }
    }

    private static func sendEcho(fd: Int32, packet: [UInt8], to addr4: inout in_addr) -> Int {
        packet.withUnsafeBytes { buffer in
            withUnsafePointer(to: &addr4) { addr in
                var sa = sockaddr_in()
                sa.sin_family = sa_family_t(AF_INET)
                sa.sin_addr = addr.pointee
                return withUnsafePointer(to: &sa) { saPointer in
                    saPointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { rebound in
                        sendto(fd, buffer.baseAddress, buffer.count, 0, rebound,
                               socklen_t(MemoryLayout<sockaddr_in>.size))
                    }
                }
            }
        }
    }

    private static func awaitEchoReply(fd: Int32, sequence: UInt16, isV6: Bool, deadline: Date) -> Bool {
        let replyType: UInt8 = isV6 ? 129 : 0
        var buffer = [UInt8](repeating: 0, count: 1500)
        while Date() < deadline {
            let received = recv(fd, &buffer, buffer.count, 0)
            guard received > 0 else { return false }
            let icmp = icmpPayload(buffer, count: received, isV6: isV6)
            // Dedicated socket, one outstanding request: an echo reply is
            // ours. The kernel rewrites the identifier, and the sequence in
            // the echoed payload is only a tie-breaker if it survived.
            guard icmp.count >= 8, icmp[0] == replyType else { continue }
            let echoedSequence = UInt16(icmp[6]) << 8 | UInt16(icmp[7])
            if echoedSequence == sequence || echoedSequence == 0 {
                return true
            }
        }
        return false
    }

    private static func icmpPacket(type: UInt8, sequence: UInt16, computeChecksum: Bool) -> [UInt8] {
        var packet: [UInt8] = [type, 0, 0, 0, 0, 0, UInt8(sequence >> 8), UInt8(sequence & 0xFF)]
        packet.append(contentsOf: pingPayload)
        if computeChecksum {
            let sum = checksum(packet)
            packet[2] = UInt8(sum >> 8)
            packet[3] = UInt8(sum & 0xFF)
        }
        return packet
    }

    /// The reply on a `SOCK_DGRAM` ICMP socket is normally the bare ICMP
    /// message, but macOS can prepend the IPv4 header — strip it when the
    /// first nibble says IPv4.
    private static func icmpPayload(_ buffer: [UInt8], count: Int, isV6: Bool) -> [UInt8] {
        guard count > 0 else { return [] }
        if !isV6, buffer[0] >> 4 == 4 {
            let headerLength = Int(buffer[0] & 0x0F) * 4
            guard count > headerLength else { return [] }
            return Array(buffer[headerLength ..< count])
        }
        return Array(buffer[0 ..< count])
    }

    private static func checksum(_ bytes: [UInt8]) -> UInt16 {
        var sum: UInt32 = 0
        var index = 0
        while index + 1 < bytes.count {
            sum += UInt32(bytes[index]) << 8 | UInt32(bytes[index + 1])
            index += 2
        }
        if index < bytes.count {
            sum += UInt32(bytes[index]) << 8
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16)
        }
        return ~UInt16(sum & 0xFFFF)
    }
}
