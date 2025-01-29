// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

interface IICS24HostErrors {
    /// @notice Packet commitment already exists
    /// @param path commitment path
    error IBCPacketCommitmentAlreadyExists(bytes path);

    /// @notice Packet acknowledgement already exists
    /// @param path commitment path
    error IBCPacketAcknowledgementAlreadyExists(bytes path);

    /// @notice Merkle prefix is invalid
    /// @param prefix The invalid prefix
    error InvalidMerklePrefix(bytes[] prefix);

    /// @notice Multi-payload packets are not supported
    error IBCMultiPayloadPacketNotSupported();
}
