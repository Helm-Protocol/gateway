// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// contracts/QkvgEscrow.sol
//
// Helm Gateway Escrow Contract
// Base Chain (Ethereum L2) deployment target
//
// Security: Reentrancy → nonReentrant + CEI pattern
//           Front-running → Commit-Reveal (Phase 2 signature based)
//
// Settlement: Daily at 00:00 UTC via Gateway cron
// Treasury:   0x7e0118A33202c03949167853b05631baC0fA9756 (hardcoded)

import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/cryptography/MerkleProof.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

contract QkvgEscrow is ReentrancyGuard, Ownable {
    using SafeERC20 for IERC20;

    // ============================
    // CONSTANTS
    // ============================

    /// Helm Protocol Treasury — all fees flow here
    address public constant HELM_TREASURY = 0x7e0118A33202c03949167853b05631baC0fA9756;

    /// BNKR token on Base Chain (set at deploy time)
    IERC20 public immutable BNKR;

    /// Settlement interval: 1 day
    uint256 public constant SETTLE_INTERVAL = 1 days;

    // ============================
    // STATE
    // ============================

    /// Agent ETH deposits
    mapping(address => uint256) public ethDeposits;

    /// Agent BNKR deposits
    mapping(address => uint256) public bnkrDeposits;

    /// Settled batch roots (replay protection)
    mapping(bytes32 => bool) public settledBatches;

    /// Last settlement timestamp
    uint256 public lastSettledAt;

    /// Gateway address (settlement authority)
    address public gateway;

    /// Yield protocol (Lido integration)
    address public yieldProtocol;

    /// TVL stake ratio (default 80%)
    uint256 public stakeRatio = 80;

    // ============================
    // EVENTS
    // ============================

    event EthDeposited(address indexed agent, uint256 amount);
    event BnkrDeposited(address indexed agent, uint256 amount);
    event EthWithdrawn(address indexed agent, uint256 amount);
    event BnkrWithdrawn(address indexed agent, uint256 amount);
    event DailySettled(bytes32 indexed merkleRoot, uint256 ethAmount, uint256 bnkrAmount, uint256 agentCount, uint256 settledAt);
    event GatewayUpdated(address indexed oldGateway, address indexed newGateway);

    // ============================
    // MODIFIERS
    // ============================

    modifier onlyGateway() {
        require(msg.sender == gateway, "QkvgEscrow: caller is not gateway");
        _;
    }

    modifier dailySettleReady() {
        require(
            block.timestamp >= lastSettledAt + SETTLE_INTERVAL,
            "QkvgEscrow: settle interval not elapsed (1 day)"
        );
        _;
    }

    // ============================
    // CONSTRUCTOR
    // ============================

    constructor(
        address _gateway,
        address _bnkrToken,
        address _yieldProtocol
    ) Ownable(msg.sender) {
        require(_gateway    != address(0), "gateway cannot be zero");
        require(_bnkrToken  != address(0), "BNKR token cannot be zero");

        gateway       = _gateway;
        BNKR          = IERC20(_bnkrToken);
        yieldProtocol = _yieldProtocol;
        lastSettledAt = block.timestamp;
    }

    // ============================
    // DEPOSIT — ETH
    // ============================

    /// Agent deposits ETH
    function depositEth() external payable nonReentrant {
        require(msg.value > 0, "deposit amount must be positive");
        ethDeposits[msg.sender] += msg.value;
        emit EthDeposited(msg.sender, msg.value);
    }

    /// Withdraw ETH
    function withdrawEth(uint256 amount) external nonReentrant {
        require(ethDeposits[msg.sender] >= amount, "insufficient ETH deposit");
        ethDeposits[msg.sender] -= amount;
        (bool ok, ) = payable(msg.sender).call{value: amount}("");
        require(ok, "ETH withdraw failed");
        emit EthWithdrawn(msg.sender, amount);
    }

    // ============================
    // DEPOSIT — BNKR
    // ============================

    /// Agent deposits BNKR tokens
    /// Requires: BNKR.approve(escrowAddress, amount) first
    function depositBnkr(uint256 amount) external nonReentrant {
        require(amount > 0, "deposit amount must be positive");
        BNKR.safeTransferFrom(msg.sender, address(this), amount);
        bnkrDeposits[msg.sender] += amount;
        emit BnkrDeposited(msg.sender, amount);
    }

    /// Withdraw BNKR
    function withdrawBnkr(uint256 amount) external nonReentrant {
        require(bnkrDeposits[msg.sender] >= amount, "insufficient BNKR deposit");
        bnkrDeposits[msg.sender] -= amount;
        BNKR.safeTransfer(msg.sender, amount);
        emit BnkrWithdrawn(msg.sender, amount);
    }

    // ============================
    // DAILY SETTLEMENT
    // ============================

    /// Daily settlement — callable once per 24h by Gateway cron
    /// Settles 24h worth of x402 tickets via Merkle proof
    /// All proceeds → HELM_TREASURY (0x7e0118A33202c03949167853b05631baC0fA9756)
    ///
    /// @param merkleRoot   Merkle root of 24h x402 tickets
    /// @param ethAmount    Total ETH to transfer to treasury
    /// @param bnkrAmount   Total BNKR to transfer to treasury
    /// @param agentCount   Number of agents settled
    /// @param proof        Merkle proof
    function settleDaily(
        bytes32 merkleRoot,
        uint256 ethAmount,
        uint256 bnkrAmount,
        uint256 agentCount,
        bytes32[] calldata proof
    ) external onlyGateway nonReentrant dailySettleReady {
        // === CHECKS ===
        require(!settledBatches[merkleRoot], "batch already settled");
        require(ethAmount > 0 || bnkrAmount > 0, "nothing to settle");
        require(address(this).balance >= ethAmount, "insufficient ETH balance");
        require(BNKR.balanceOf(address(this)) >= bnkrAmount, "insufficient BNKR balance");

        // Merkle proof verification
        bytes32 leaf = keccak256(abi.encodePacked(merkleRoot, ethAmount, bnkrAmount, agentCount));
        require(MerkleProof.verify(proof, merkleRoot, leaf), "invalid merkle proof");

        // === EFFECTS ===
        settledBatches[merkleRoot] = true;
        lastSettledAt = block.timestamp;

        // === INTERACTIONS — treasury receives all ===
        if (ethAmount > 0) {
            (bool ok, ) = payable(HELM_TREASURY).call{value: ethAmount}("");
            require(ok, "ETH treasury transfer failed");
        }
        if (bnkrAmount > 0) {
            BNKR.safeTransfer(HELM_TREASURY, bnkrAmount);
        }

        emit DailySettled(merkleRoot, ethAmount, bnkrAmount, agentCount, block.timestamp);
    }

    // ============================
    // YIELD (STAKING)
    // ============================

    /// Stake idle ETH via Lido (called by Gateway periodically)
    function stakeIdleFunds() external onlyGateway {
        if (yieldProtocol == address(0)) return;
        uint256 idle = address(this).balance * stakeRatio / 100;
        if (idle == 0) return;
        (bool ok, ) = yieldProtocol.call{value: idle}(
            abi.encodeWithSignature("submit(address)", address(0))
        );
        require(ok, "staking failed");
    }

    // ============================
    // VIEWS
    // ============================

    function getEthDeposit(address agent) external view returns (uint256) {
        return ethDeposits[agent];
    }

    function getBnkrDeposit(address agent) external view returns (uint256) {
        return bnkrDeposits[agent];
    }

    function getEthTVL() external view returns (uint256) {
        return address(this).balance;
    }

    function getBnkrTVL() external view returns (uint256) {
        return BNKR.balanceOf(address(this));
    }

    function nextSettleAt() external view returns (uint256) {
        return lastSettledAt + SETTLE_INTERVAL;
    }

    // ============================
    // ADMIN
    // ============================

    function updateGateway(address newGateway) external onlyOwner {
        require(newGateway != address(0), "invalid gateway");
        emit GatewayUpdated(gateway, newGateway);
        gateway = newGateway;
    }

    function updateStakeRatio(uint256 newRatio) external onlyOwner {
        require(newRatio <= 100, "ratio cannot exceed 100");
        stakeRatio = newRatio;
    }

    receive() external payable {
        ethDeposits[msg.sender] += msg.value;
        emit EthDeposited(msg.sender, msg.value);
    }
}
