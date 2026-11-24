import AppKit

extension UpNextPanel {
    static let reorderType = NSPasteboard.PasteboardType("com.befeast.ok-player.queue-row")

    func tableView(_ tableView: NSTableView, pasteboardWriterForRow row: Int) -> (any NSPasteboardWriting)? {
        guard let entry = list.row(at: row), !entry.isCurrent else { return nil }
        let item = NSPasteboardItem()
        item.setString(String(row), forType: Self.reorderType)
        return item
    }

    func tableView(_ tableView: NSTableView, draggingSession session: NSDraggingSession,
                   willBeginAt screenPoint: NSPoint, forRowIndexes rowIndexes: IndexSet) {
        if let row = rowIndexes.first { drag = (row, list) }
        session.animatesToStartingPositionsOnCancelOrFail = true
    }

    func tableView(_ tableView: NSTableView, draggingSession session: NSDraggingSession,
                   endedAt screenPoint: NSPoint, operation: NSDragOperation) {
        drag = nil
        showInsertion(before: nil)
    }

    func tableView(_ tableView: NSTableView, validateDrop info: any NSDraggingInfo,
                   proposedRow row: Int, proposedDropOperation operation: NSTableView.DropOperation) -> NSDragOperation {
        guard info.draggingSource as? NSTableView === tableView else { return [] }
        let insertion: Int
        if operation == .on, row >= 0, row < tableView.numberOfRows {
            let location = tableView.convert(info.draggingLocation, from: nil)
            insertion = location.y < tableView.rect(ofRow: row).midY ? row : row + 1
        } else {
            insertion = row
        }
        guard previewReorder(before: insertion) else { return [] }
        tableView.setDropRow(insertion, dropOperation: .above)
        return .move
    }

    func tableView(_ tableView: NSTableView, acceptDrop info: any NSDraggingInfo,
                   row: Int, dropOperation: NSTableView.DropOperation) -> Bool {
        guard info.draggingSource as? NSTableView === tableView, dropOperation == .above else { return false }
        return completeReorder(before: row)
    }

    @discardableResult
    func previewReorder(before boundary: Int) -> Bool {
        let allowed = reorderCommand(before: boundary) != nil
        showInsertion(before: allowed ? boundary : nil)
        return allowed
    }

    @discardableResult
    func completeReorder(before boundary: Int) -> Bool {
        defer { showInsertion(before: nil) }
        guard let command = reorderCommand(before: boundary) else { return false }
        dispatch?(command)
        return true
    }

    private func reorderCommand(before boundary: Int) -> Command? {
        guard let drag, drag.list == list else { return nil }
        return list.moving(from: drag.row, before: boundary)
    }

    func showInsertion(before boundary: Int?) {
        guard let boundary, let clip = insertionLine.superview, tableView.numberOfRows > 0 else {
            insertionLine.isHidden = true
            return
        }
        let index = min(boundary, tableView.numberOfRows - 1)
        let row = tableView.rect(ofRow: index)
        let y = boundary == tableView.numberOfRows ? row.maxY : row.minY
        insertionLine.frame = tableView.convert(
            NSRect(x: 2, y: y - 1, width: tableView.bounds.width - 4, height: 2), to: clip)
        insertionLine.isHidden = false
    }
}
