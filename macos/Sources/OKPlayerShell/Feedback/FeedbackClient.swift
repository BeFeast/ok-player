import COKPCore
import Foundation

struct FeedbackReport: Codable {
    enum Kind: String, Codable { case bug, feature }

    struct Metadata: Codable {
        var version: String
        var build: String
        var source: String
        var os: String
        var osVersion: String
        var arch: String
        var channel: String
    }

    struct Diagnostics: Codable {
        var playback: String
        var fullscreen: Bool
        var width: Int
        var height: Int
    }

    var text: String
    var kind: Kind
    var reporterName: String
    var meta: Metadata
    var diagnostics: Diagnostics?
    var screenshot: Data?
}

struct FeedbackForm: Decodable {
    struct Draft: Decodable {
        var token: String
        var report: FeedbackReport
        var message: String
    }

    var privacy: String
    var reporterName: String
    var draft: Draft?
}

struct FeedbackIssue: Decodable {
    var issueNumber: UInt64
    var issueUrl: URL
}

struct FeedbackClient: Sendable {
    let directory: String
    var testPort: UInt16 = 0

    struct Failure: LocalizedError {
        var message: String
        var errorDescription: String? { message }
    }

    private struct Response<Value: Decodable>: Decodable {
        let value: Value?
        let error: String?
    }

    private struct Submission: Encodable {
        let token: String?
        let report: FeedbackReport
    }

    func form() throws -> FeedbackForm {
        try decode(okp_feedback_form(directory, testPort))
    }

    func discard(_ token: String) throws {
        let _: Bool = try decode(okp_feedback_discard(directory, testPort, token))
    }

    func submit(_ report: FeedbackReport, token: String?) throws -> FeedbackIssue {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let data = try encoder.encode(Submission(token: token, report: report))
        let input = String(decoding: data, as: UTF8.self)
        return try decode(okp_feedback_submit(directory, testPort, input))
    }

    private func decode<Value: Decodable>(_ pointer: UnsafeMutablePointer<CChar>?) throws -> Value {
        guard let pointer else { throw Failure(message: "Could not read feedback.") }
        defer { okp_feedback_free(pointer) }
        let data = Data(bytes: pointer, count: strlen(pointer))
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let response = try decoder.decode(Response<Value>.self, from: data)
        guard let value = response.value else { throw Failure(message: response.error ?? "Could not read feedback.") }
        return value
    }
}
