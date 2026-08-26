import Foundation

extension Probe {
    private static let captivePortalURLString = "http://captive.apple.com/hotspot-detect.html"
    private static let captivePortalTimeout: TimeInterval = 10

    /// Whether something between this machine and the internet is
    /// intercepting plain HTTP — determined by fetching Apple's own
    /// hotspot-detect endpoint over HTTP (not HTTPS: the interception has
    /// to be visible) and checking for its `Success` marker. A transport
    /// failure is inconclusive (`.unknown`), not evidence of a portal.
    public static func checkCaptivePortal() async -> CaptivePortalStatus {
        await checkCaptivePortal(fetch: liveCaptivePortalFetch)
    }

    static func checkCaptivePortal(
        fetch: @Sendable (URL) async throws -> (Data, URLResponse)
    ) async -> CaptivePortalStatus {
        guard let url = URL(string: captivePortalURLString) else { return .unknown }
        do {
            let (data, response) = try await fetch(url)
            guard let http = response as? HTTPURLResponse else { return .unknown }
            // Malformed bytes in a portal's HTML should degrade to replacement
            // characters, not abort classification — the lossy initializer is
            // deliberate here.
            // swiftlint:disable:next optional_data_string_conversion
            let body = String(decoding: data, as: UTF8.self)
            return classifyCaptivePortalResponse(status: http.statusCode, body: body)
        } catch {
            return .unknown
        }
    }

    private static func liveCaptivePortalFetch(_ url: URL) async throws -> (Data, URLResponse) {
        var request = URLRequest(url: url)
        request.timeoutInterval = captivePortalTimeout
        request.cachePolicy = .reloadIgnoringLocalAndRemoteCacheData

        let session = URLSession(configuration: .ephemeral)
        defer { session.finishTasksAndInvalidate() }

        return try await session.data(for: request, delegate: RedirectRefuser())
    }

    /// A 200 carrying `Success` means nothing intercepted the request.
    /// Anything else — rewritten body, redirect, other status — is a
    /// portal (or something else altering the response).
    static func classifyCaptivePortalResponse(status: Int, body: String) -> CaptivePortalStatus {
        status == 200 && body.contains("Success") ? .clear : .detected
    }
}

/// Refuses HTTP redirects so a captive portal's `302 -> login page` is
/// seen as the 302 it is, rather than followed to a page that might
/// coincidentally 200.
private final class RedirectRefuser: NSObject, URLSessionTaskDelegate, Sendable {
    func urlSession(
        _: URLSession,
        task _: URLSessionTask,
        willPerformHTTPRedirection _: HTTPURLResponse,
        newRequest _: URLRequest,
        completionHandler: @escaping (URLRequest?) -> Void
    ) {
        completionHandler(nil)
    }
}
