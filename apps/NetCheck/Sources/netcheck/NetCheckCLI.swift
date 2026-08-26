import ArgumentParser
import Foundation
import NetStatus

/// Release tag when built by CI (`NETCHECK_VERSION`), `dev` otherwise.
let netcheckVersion = ProcessInfo.processInfo.environment["NETCHECK_VERSION"] ?? "dev"

@main
struct NetCheck: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "netcheck",
        abstract: "Holistic view of macOS network status. Every subcommand prints JSON.",
        version: netcheckVersion,
        subcommands: [
            Status.self, Interfaces.self, Dns.self, Vpn.self, Proxy.self, Wifi.self,
            Path.self, Captive.self, Connection.self, Ping.self, Resolve.self, Connect.self,
        ],
        defaultSubcommand: Status.self
    )
}

func printJSON(_ value: some Encodable) {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
    encoder.keyEncodingStrategy = .convertToSnakeCase
    guard let data = try? encoder.encode(value), let string = String(data: data, encoding: .utf8)
    else { return }
    print(string)
}

struct Status: AsyncParsableCommand {
    static let configuration = CommandConfiguration(abstract: "Full network status snapshot")
    func run() async throws {
        try await printJSON(NetStatus.collect())
    }
}

struct Interfaces: AsyncParsableCommand {
    static let configuration = CommandConfiguration(abstract: "Network interfaces")
    func run() async throws {
        printJSON(Probe.listInterfaces())
    }
}

struct Dns: AsyncParsableCommand {
    static let configuration = CommandConfiguration(abstract: "DNS resolver configuration")
    func run() async throws {
        printJSON(Probe.listResolvers())
    }
}

struct Vpn: AsyncParsableCommand {
    static let configuration = CommandConfiguration(abstract: "VPN / tunnel status")
    func run() async throws {
        printJSON(Probe.vpnStatus(Probe.listInterfaces()))
    }
}

struct Proxy: AsyncParsableCommand {
    static let configuration = CommandConfiguration(abstract: "Proxy configuration")
    func run() async throws {
        printJSON(Probe.proxyConfig())
    }
}

struct Wifi: AsyncParsableCommand {
    static let configuration = CommandConfiguration(abstract: "Wi-Fi diagnostics")
    func run() async throws {
        await printJSON(WifiStatus(identity: Probe.wifiIdentity(), radio: Probe.wifiRadio()))
    }
}

struct Path: AsyncParsableCommand {
    static let configuration = CommandConfiguration(abstract: "Primary network path (NWPath)")
    func run() async throws {
        await printJSON(Probe.pathStatus())
    }
}

struct Captive: AsyncParsableCommand {
    static let configuration = CommandConfiguration(abstract: "Captive-portal status")
    func run() async throws {
        await printJSON(Probe.checkCaptivePortal())
    }
}

struct Connection: AsyncParsableCommand {
    static let configuration = CommandConfiguration(
        abstract: "Connection tier derived from DNS/ICMP/TCP probes"
    )
    func run() async throws {
        await printJSON(NetStatus.connectivityOnly())
    }
}

struct Ping: AsyncParsableCommand {
    static let configuration = CommandConfiguration(abstract: "Ping one or more hosts")
    @Argument(help: "IP addresses to ping (defaults to well-known public DNS IPs)")
    var targets: [String] = Defaults.pingTargets

    func run() async throws {
        await printJSON(withOrderedResults(targets) { await Probe.ping($0) })
    }
}

struct Resolve: AsyncParsableCommand {
    static let configuration = CommandConfiguration(abstract: "Resolve one or more domains")
    @Argument(help: "domains to resolve (defaults to a well-known domain list)")
    var domains: [String] = Defaults.resolutionTargets

    func run() async throws {
        await printJSON(withOrderedResults(domains) { await Probe.resolve($0) })
    }
}

struct Connect: AsyncParsableCommand {
    static let configuration = CommandConfiguration(abstract: "TCP connect to one or more hosts")
    @Argument(help: "hosts to connect to (defaults to a well-known domain list)")
    var targets: [String] = Defaults.resolutionTargets
    @Option(name: .shortAndLong, help: "port")
    var port: UInt16 = 443

    func run() async throws {
        let port = port
        await printJSON(withOrderedResults(targets) { await Probe.connect($0, port: port) })
    }
}

/// Runs `probe` over `items` concurrently, returning results in input order.
private func withOrderedResults<Result: Sendable>(
    _ items: [String], _ probe: @Sendable @escaping (String) async -> Result
) async -> [Result] {
    await withTaskGroup(of: (Int, Result).self) { group in
        for (index, item) in items.enumerated() {
            group.addTask { await (index, probe(item)) }
        }
        var results = [Result?](repeating: nil, count: items.count)
        for await (index, result) in group {
            results[index] = result
        }
        return results.compactMap(\.self)
    }
}
