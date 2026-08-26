import Darwin
import Foundation

extension Probe {
    /// The default gateway's IP addresses (IPv4 first, then IPv6), or an
    /// empty list when the default route can't be determined:
    ///  1. ask the OS which local address it would use for outbound traffic,
    ///  2. dump the routing table and collect the gateway of every
    ///     `RTF_GATEWAY` default route, keyed by outgoing interface index,
    ///  3. return the gateways on the interface that owns the local address.
    public static func defaultGatewayIps() -> [String] {
        guard let localAddress = localOutboundAddress() else { return [] }
        guard let index = Self.interfaceIndex(owning: localAddress) else { return [] }

        let gateways = Self.defaultRouteGateways()
        guard let device = gateways[index] else { return [] }
        return device.v4 + device.v6
    }

    // MARK: outbound address

    /// The source address the kernel selects to reach a non-routable
    /// destination — no packets are sent. Tries IPv4 then IPv6, matching
    private static func localOutboundAddress() -> String? {
        for (family, probeTarget) in [
            (AF_INET, "10.254.254.254"),
            (AF_INET6, "fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"),
        ] {
            if let address = sourceAddress(family: family, reaching: probeTarget) {
                return address
            }
        }
        return nil
    }

    private static func sourceAddress(family: Int32, reaching target: String) -> String? {
        var hints = addrinfo()
        hints.ai_family = family
        hints.ai_socktype = SOCK_DGRAM
        hints.ai_flags = AI_NUMERICHOST

        var result: UnsafeMutablePointer<addrinfo>?
        guard getaddrinfo(target, "1", &hints, &result) == 0, let info = result else { return nil }
        defer { freeaddrinfo(info) }

        let fd = socket(info.pointee.ai_family, info.pointee.ai_socktype, info.pointee.ai_protocol)
        guard fd >= 0 else { return nil }
        defer { close(fd) }
        guard Darwin.connect(fd, info.pointee.ai_addr, info.pointee.ai_addrlen) == 0 else {
            return nil
        }

        var local = sockaddr_storage()
        var length = socklen_t(MemoryLayout<sockaddr_storage>.size)
        let ok = withUnsafeMutablePointer(to: &local) { storage in
            storage.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(fd, $0, &length) == 0 }
        }
        guard ok else { return nil }
        return withUnsafePointer(to: &local) { storage in
            storage.withMemoryRebound(to: sockaddr.self, capacity: 1) { Self.ipString($0) }
        }
    }

    private static func interfaceIndex(owning address: String) -> UInt32? {
        for iface in listInterfaces() where iface.addresses.contains(where: { addr in
            (addr.split(separator: "/", maxSplits: 1).first.map(String.init) ?? addr) == address
        }) {
            let index = if_nametoindex(iface.name)
            if index != 0 {
                return index
            }
        }
        return nil
    }

    // MARK: routing table

    private struct GatewayAddresses {
        var v4: [String] = []
        var v6: [String] = []
    }

    /// `ifindex -> gateway addresses` for every `RTF_GATEWAY` route whose
    /// destination is a default route (prefix 0). Only the gateway address
    /// is collected — the neighbour/MAC table the full route dump also
    /// carries is not needed here.
    private static func defaultRouteGateways() -> [UInt32: GatewayAddresses] {
        guard let buffer = sysctlRouteDump() else { return [:] }
        var out: [UInt32: GatewayAddresses] = [:]

        let headerSize = MemoryLayout<rt_msghdr>.size
        buffer.withUnsafeBytes { raw in
            var offset = 0
            while offset + headerSize <= raw.count {
                let header = raw.loadUnaligned(fromByteOffset: offset, as: rt_msghdr.self)
                let messageLength = Int(header.rtm_msglen)
                guard messageLength >= headerSize, offset + messageLength <= raw.count else { break }
                defer { offset += messageLength }

                guard header.rtm_version == RTM_VERSION else { continue }
                guard header.rtm_flags & RTF_WASCLONED == 0 else { continue }
                guard header.rtm_flags & RTF_GATEWAY != 0 else { continue }

                let addressBlock = UnsafeRawBufferPointer(
                    rebasing: raw[(offset + headerSize) ..< (offset + messageLength)]
                )
                let slots = Self.routeSockaddrs(header.rtm_addrs, in: addressBlock)

                guard Self.isDefaultRoute(destination: slots[Int(RTAX_DST)],
                                          netmask: slots[Int(RTAX_NETMASK)],
                                          flags: header.rtm_flags),
                    let gatewaySlot = slots[Int(RTAX_GATEWAY)],
                    let gateway = Self.routeGatewayAddress(gatewaySlot)
                else { continue }

                let index = UInt32(header.rtm_index)
                if gateway.contains(":") {
                    if !(out[index]?.v6.contains(gateway) ?? false) {
                        out[index, default: GatewayAddresses()].v6.append(gateway)
                    }
                } else if !(out[index]?.v4.contains(gateway) ?? false) {
                    out[index, default: GatewayAddresses()].v4.append(gateway)
                }
            }
        }
        return out
    }

    private static func sysctlRouteDump() -> [UInt8]? {
        var mib: [Int32] = [CTL_NET, PF_ROUTE, 0, 0, NET_RT_DUMP, 0]
        var length = 0
        guard sysctl(&mib, UInt32(mib.count), nil, &length, nil, 0) == 0, length > 0 else { return nil }
        var buffer = [UInt8](repeating: 0, count: length)
        let ok = buffer.withUnsafeMutableBytes { raw in
            sysctl(&mib, UInt32(mib.count), raw.baseAddress, &length, nil, 0) == 0
        }
        guard ok else { return nil }
        buffer.removeLast(buffer.count - length)
        return buffer
    }

    /// The `sockaddr` pointer for each `RTAX_*` slot present in `mask`,
    /// stepping by the 4-byte-aligned `sa_len` the routing socket uses.
    static func routeSockaddrs(
        _ mask: Int32, in block: UnsafeRawBufferPointer
    ) -> [UnsafePointer<sockaddr>?] {
        var slots = [UnsafePointer<sockaddr>?](repeating: nil, count: Int(RTAX_MAX))
        guard let base = block.baseAddress else { return slots }
        var offset = 0
        for slot in 0 ..< Int(RTAX_MAX) where mask & (1 << slot) != 0 {
            guard offset < block.count else { break }
            let saLen = Int(block[offset])
            guard offset + (saLen == 0 ? 4 : saLen) <= block.count else { break }
            slots[slot] = base.advanced(by: offset).assumingMemoryBound(to: sockaddr.self)
            offset += saLen == 0 ? 4 : (saLen + 3) & ~3
        }
        return slots
    }

    private static func isDefaultRoute(
        destination: UnsafePointer<sockaddr>?,
        netmask: UnsafePointer<sockaddr>?,
        flags: Int32
    ) -> Bool {
        guard let destination else { return false }
        let family = Int32(destination.pointee.sa_family)
        guard family == AF_INET || family == AF_INET6 else { return false }

        if let netmask {
            if netmask.pointee.sa_len == 0 {
                return true
            }
            return Self.prefixLength(netmask, family: family) == 0
        }
        // No netmask slot: a host route is prefix 32/128, anything else 0.
        return flags & RTF_HOST == 0
    }

    private static func routeGatewayAddress(_ gateway: UnsafePointer<sockaddr>) -> String? {
        switch Int32(gateway.pointee.sa_family) {
        case AF_INET where Int(gateway.pointee.sa_len) >= MemoryLayout<sockaddr_in>.size:
            ipString(gateway)
        case AF_INET6 where Int(gateway.pointee.sa_len) >= MemoryLayout<sockaddr_in6>.size:
            ipString(gateway).map(normalizeScopedV6)
        default:
            nil
        }
    }

    /// Strips the scope id some link-local gateway addresses embed in the
    /// second 16-bit group (`fe80:SCOPE::…`).
    private static func normalizeScopedV6(_ address: String) -> String {
        var storage = in6_addr()
        guard inet_pton(AF_INET6, address, &storage) == 1 else { return address }
        let bytes = withUnsafeBytes(of: &storage) { Array($0) }
        let isUnicastLinkLocal = bytes[0] == 0xFE && (bytes[1] & 0xC0) == 0x80
        let isLocalScopeMulticast = bytes[0] == 0xFF && (bytes[1] & 0x0F == 0x01 || bytes[1] & 0x0F == 0x02)
        guard isUnicastLinkLocal || isLocalScopeMulticast else { return address }

        var cleaned = storage
        withUnsafeMutableBytes(of: &cleaned) { raw in
            raw[2] = 0
            raw[3] = 0
        }
        var buffer = [CChar](repeating: 0, count: Int(INET6_ADDRSTRLEN))
        return inet_ntop(AF_INET6, &cleaned, &buffer, socklen_t(buffer.count))
            .map { String(cString: $0) } ?? address
    }
}
