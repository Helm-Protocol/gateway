// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// contracts/HelmEscrow.sol  v2
//
// Helm Gateway Escrow Contract — Base Chain (Ethereum L2)
//
// ╔══════════════════════════════════════════════════════════╗
// ║  REVENUE FLOWS (all → treasury by default)               ║
// ║                                                          ║
// ║  API calls (Oracle/Shield/Search/DeFi/Identity)          ║
// ║    80% → HELM_TREASURY                                   ║
// ║    20% → referring agent wallet                          ║
// ║                                                          ║
// ║  DID registration: 0.001 ETH flat → 100% treasury       ║
// ║  Escrow settlement: 2% of amount  → 100% treasury       ║
// ║  Staking yield cut: 10% of yield  → 100% treasury       ║
// ║  x402 daily batch: 100% → treasury                      ║
// ╚══════════════════════════════════════════════════════════╝
//
// Security: Reentrancy → nonReentrant + CEI pattern
// Settlement: Daily at 00:00 UTC via Gateway cron
// Treasury:   0x7e0118A33202c03949167853b05631baC0fA9756

import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/cryptography/MerkleProof.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";

contract HelmEscrow is ReentrancyGuard, Ownable {
    using SafeERC20 for IERC20;

    // ============================
    // CONSTANTS
    // ============================

    /// Helm Protocol Treasury — primary revenue destination
    address public constant HELM_TREASURY = 0x7e0118A33202c03949167853b05631baC0fA9756;

    /// BNKR token on Base Chain
    IERC20 public immutable BNKR;

    /// Settlement interval: 1 day
    uint256 public constant SETTLE_INTERVAL = 1 days;

    // ── Protocol Fees ────────────────────────────────────────────

    /// DID registration flat fee: 0.001 ETH → 100% treasury
    uint256 public constant DID_REGISTRATION_FEE = 0.001 ether;

    /// Escrow agent-to-agent settlement fee: 2% → 100% treasury
    uint256 public constant ESCROW_SETTLEMENT_FEE_BP = 200; // basis points

    /// Staking yield protocol cut: 10% → 100% treasury
    uint256 public constant STAKING_YIELD_CUT_BP = 1000; // basis points

    /// API revenue split: 80% treasury, 20% referrer agent
    uint256 public constant TREASURY_SHARE_BP    = 8000;
    uint256 public constant REFERRER_SHARE_BP    = 2000;

    // ── API Call Fees (in BNKR, 18-decimal) ─────────────────────
    uint256 public constant GRG_CALL_FEE        = 5e14;  // 0.0005 BNKR
    uint256 public constant ATTENTION_CALL_FEE  = 1e15;  // 0.001  BNKR
    uint256 public constant REPUTATION_FEE      = 2e14;  // 0.0002 BNKR
    uint256 public constant SYNCO_CLEAN_FEE     = 1e14;  // 0.0001 BNKR per 1k items
    uint256 public constant IDENTITY_QUERY_FEE  = 5e14;  // 0.0005 BNKR external

    // ── A/B/C/D Front API Fees ───────────────────────────────────
    uint256 public constant LLM_MARKUP_BP       = 500;   // +5% on LLM cost
    uint256 public constant SEARCH_MARKUP_BP    = 1000;  // +10% on search cost
    uint256 public constant DEFI_FEE_BP         = 10;    // 0.1% on swap value
    // ────────────────────────────────────────────────────────────

    // ============================
    // STATE
    // ============================

    /// Agent ETH pre-deposits (x402 payment channel balance)
    mapping(address => uint256) public ethDeposits;

    /// Agent BNKR pre-deposits
    mapping(address => uint256) public bnkrDeposits;

    /// Agent DID registration status
    mapping(address => bool) public registeredDids;

    /// Referring agent for each DID (earns 15% API revenue)
    mapping(address => address) public referrer;

    /// Referrer accumulated earnings (claimable)
    mapping(address => uint256) public referrerEarningsBnkr;
    mapping(address => uint256) public referrerEarningsEth;

    /// Agent-to-agent escrow (A pays B for work)
    struct AgentEscrow {
        address payer;
        address payee;
        uint256 bnkrAmount;
        uint256 deadline;
        bool    settled;
        bool    refunded;
    }
    mapping(bytes32 => AgentEscrow) public agentEscrows;

    /// Settled x402 batch roots (replay protection)
    mapping(bytes32 => bool) public settledBatches;

    /// Last x402 daily settlement timestamp
    uint256 public lastSettledAt;

    /// Gateway address (settlement authority)
    address public gateway;

    /// Yield protocol (Lido integration)
    address public yieldProtocol;

    /// TVL stake ratio for Lido (default 80%)
    uint256 public stakeRatio = 80;

    /// Cumulative protocol stats
    uint256 public totalTreasuryEth;
    uint256 public totalTreasuryBnkr;
    uint256 public totalReferrerBnkr;
    uint256 public totalDidsRegistered;

    // ============================
    // EVENTS
    // ============================

    event EthDeposited(address indexed agent, uint256 amount);
    event BnkrDeposited(address indexed agent, uint256 amount);
    event EthWithdrawn(address indexed agent, uint256 amount);
    event BnkrWithdrawn(address indexed agent, uint256 amount);

    event DidRegistered(address indexed agent, address indexed ref, uint256 fee);

    event ApiCharged(
        address indexed agent,
        address indexed ref,
        string  endpoint,
        uint256 totalFee,
        uint256 treasuryShare,
        uint256 referrerShare
    );

    event AgentEscrowCreated(bytes32 indexed escrowId, address payer, address payee, uint256 amount);
    event AgentEscrowSettled(bytes32 indexed escrowId, uint256 fee, uint256 netPayee);
    event AgentEscrowRefunded(bytes32 indexed escrowId, uint256 amount);

    event StakingYieldCut(address indexed staker, uint256 yield_, uint256 cut);

    event DailySettled(
        bytes32 indexed merkleRoot,
        uint256 ethAmount,
        uint256 bnkrAmount,
        uint256 agentCount,
        uint256 settledAt
    );
    event ReferrerClaimed(address indexed ref, uint256 bnkr, uint256 eth);
    event GatewayUpdated(address indexed oldGateway, address indexed newGateway);

    // ============================
    // MODIFIERS
    // ============================

    modifier onlyGateway() {
        require(msg.sender == gateway, "not gateway");
        _;
    }

    modifier dailySettleReady() {
        require(
            block.timestamp >= lastSettledAt + SETTLE_INTERVAL,
            "settle: 1 day interval not elapsed"
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
        require(_gateway   != address(0), "gateway zero");
        require(_bnkrToken != address(0), "BNKR zero");
        gateway       = _gateway;
        BNKR          = IERC20(_bnkrToken);
        yieldProtocol = _yieldProtocol;
        lastSettledAt = block.timestamp;
    }

    // ============================
    // DID REGISTRATION
    // fee: 0.001 ETH → 100% treasury
    // ============================

    /// Register a DID. First time only. Flat 0.001 ETH fee.
    /// @param _referrer  Agent that introduced this caller (earns 15% of future API fees)
    function registerDid(address _referrer) external payable nonReentrant {
        require(!registeredDids[msg.sender], "DID already registered");
        require(msg.value >= DID_REGISTRATION_FEE, "insufficient DID registration fee");

        registeredDids[msg.sender] = true;
        totalDidsRegistered++;

        if (_referrer != address(0) && _referrer != msg.sender && registeredDids[_referrer]) {
            referrer[msg.sender] = _referrer;
        }

        // 100% of DID fee → treasury
        uint256 fee = DID_REGISTRATION_FEE;
        (bool ok, ) = payable(HELM_TREASURY).call{value: fee}("");
        require(ok, "treasury transfer failed");
        totalTreasuryEth += fee;

        // Refund any overpayment
        if (msg.value > fee) {
            (bool refund, ) = payable(msg.sender).call{value: msg.value - fee}("");
            require(refund, "refund failed");
        }

        emit DidRegistered(msg.sender, referrer[msg.sender], fee);
    }

    // ============================
    // DEPOSIT — ETH (x402 payment channel)
    // ============================

    function depositEth() external payable nonReentrant {
        require(msg.value > 0, "zero deposit");
        ethDeposits[msg.sender] += msg.value;
        emit EthDeposited(msg.sender, msg.value);
    }

    function withdrawEth(uint256 amount) external nonReentrant {
        require(ethDeposits[msg.sender] >= amount, "insufficient ETH");
        ethDeposits[msg.sender] -= amount;
        (bool ok, ) = payable(msg.sender).call{value: amount}("");
        require(ok, "withdraw failed");
        emit EthWithdrawn(msg.sender, amount);
    }

    // ============================
    // DEPOSIT — BNKR
    // ============================

    function depositBnkr(uint256 amount) external nonReentrant {
        require(amount > 0, "zero deposit");
        BNKR.safeTransferFrom(msg.sender, address(this), amount);
        bnkrDeposits[msg.sender] += amount;
        emit BnkrDeposited(msg.sender, amount);
    }

    function withdrawBnkr(uint256 amount) external nonReentrant {
        require(bnkrDeposits[msg.sender] >= amount, "insufficient BNKR");
        bnkrDeposits[msg.sender] -= amount;
        BNKR.safeTransfer(msg.sender, amount);
        emit BnkrWithdrawn(msg.sender, amount);
    }

    // ============================
    // API CHARGE (80% treasury, 20% referrer)
    // called by gateway on each API call
    // ============================

    /// Deduct API fee from agent's BNKR deposit.
    /// 80% → treasury, 20% → referrer (if any)
    function chargeApi(
        address agent,
        string calldata endpoint,
        uint256 fee
    ) external onlyGateway nonReentrant {
        require(bnkrDeposits[agent] >= fee, "insufficient BNKR for API fee");

        bnkrDeposits[agent] -= fee;

        uint256 refShare     = (fee * REFERRER_SHARE_BP) / 10_000;
        uint256 treasuryShare;

        address ref = referrer[agent];
        if (ref != address(0)) {
            referrerEarningsBnkr[ref] += refShare;
            totalReferrerBnkr += refShare;
            treasuryShare = fee - refShare;
        } else {
            // No referrer → 100% to treasury
            treasuryShare = fee;
        }

        BNKR.safeTransfer(HELM_TREASURY, treasuryShare);
        totalTreasuryBnkr += treasuryShare;

        emit ApiCharged(agent, ref, endpoint, fee, treasuryShare, refShare);
    }

    // ============================
    // REFERRER EARNINGS — CLAIM
    // ============================

    /// Referrer agent claims accumulated 15% earnings.
    function claimReferrerEarnings() external nonReentrant {
        uint256 bnkr = referrerEarningsBnkr[msg.sender];
        uint256 eth  = referrerEarningsEth[msg.sender];
        require(bnkr > 0 || eth > 0, "nothing to claim");

        referrerEarningsBnkr[msg.sender] = 0;
        referrerEarningsEth[msg.sender]  = 0;

        if (bnkr > 0) BNKR.safeTransfer(msg.sender, bnkr);
        if (eth > 0) {
            (bool ok, ) = payable(msg.sender).call{value: eth}("");
            require(ok, "ETH claim failed");
        }

        emit ReferrerClaimed(msg.sender, bnkr, eth);
    }

    // ============================
    // AGENT-TO-AGENT ESCROW
    // fee: 2% of settled amount → 100% treasury
    // ============================

    /// Create an escrow between two agents.
    function createAgentEscrow(
        bytes32 escrowId,
        address payee,
        uint256 bnkrAmount,
        uint256 ttlSeconds
    ) external nonReentrant {
        require(!agentEscrows[escrowId].settled, "escrow exists");
        require(bnkrDeposits[msg.sender] >= bnkrAmount, "insufficient BNKR");
        require(payee != address(0) && payee != msg.sender, "invalid payee");

        bnkrDeposits[msg.sender] -= bnkrAmount;

        agentEscrows[escrowId] = AgentEscrow({
            payer:     msg.sender,
            payee:     payee,
            bnkrAmount: bnkrAmount,
            deadline:  block.timestamp + ttlSeconds,
            settled:   false,
            refunded:  false
        });

        emit AgentEscrowCreated(escrowId, msg.sender, payee, bnkrAmount);
    }

    /// Settle escrow — gateway confirms work was delivered.
    /// 2% fee → treasury, 98% → payee
    function settleAgentEscrow(bytes32 escrowId) external onlyGateway nonReentrant {
        AgentEscrow storage esc = agentEscrows[escrowId];
        require(!esc.settled && !esc.refunded, "already closed");
        require(block.timestamp <= esc.deadline, "escrow expired");

        esc.settled = true;

        uint256 fee     = (esc.bnkrAmount * ESCROW_SETTLEMENT_FEE_BP) / 10_000;
        uint256 netPay  = esc.bnkrAmount - fee;

        // 2% fee → treasury
        BNKR.safeTransfer(HELM_TREASURY, fee);
        totalTreasuryBnkr += fee;

        // 98% → payee's deposit
        bnkrDeposits[esc.payee] += netPay;

        emit AgentEscrowSettled(escrowId, fee, netPay);
    }

    /// Refund expired escrow back to payer.
    function refundAgentEscrow(bytes32 escrowId) external nonReentrant {
        AgentEscrow storage esc = agentEscrows[escrowId];
        require(!esc.settled && !esc.refunded, "already closed");
        require(
            block.timestamp > esc.deadline || msg.sender == esc.payer,
            "not expired / not payer"
        );

        esc.refunded = true;
        bnkrDeposits[esc.payer] += esc.bnkrAmount;

        emit AgentEscrowRefunded(escrowId, esc.bnkrAmount);
    }

    // ============================
    // STAKING YIELD CUT
    // 10% of distributed yield → treasury
    // ============================

    /// Called by Gateway when distributing staking rewards.
    /// Takes 10% cut before distributing to stakers.
    function deductStakingYieldCut(
        address staker,
        uint256 yieldAmount
    ) external onlyGateway nonReentrant returns (uint256 netYield) {
        uint256 cut = (yieldAmount * STAKING_YIELD_CUT_BP) / 10_000;
        netYield    = yieldAmount - cut;

        require(bnkrDeposits[address(this)] >= cut, "pool insufficient for yield cut");
        BNKR.safeTransfer(HELM_TREASURY, cut);
        totalTreasuryBnkr += cut;

        emit StakingYieldCut(staker, yieldAmount, cut);
    }

    // ============================
    // DAILY x402 BATCH SETTLEMENT
    // All 85% treasury share → treasury
    // ============================

    /// Daily settlement — callable once per 24h by Gateway cron.
    /// Batches all x402 off-chain tickets and settles on-chain.
    function settleDaily(
        bytes32 merkleRoot,
        uint256 ethAmount,
        uint256 bnkrAmount,
        uint256 agentCount,
        bytes32[] calldata proof
    ) external onlyGateway nonReentrant dailySettleReady {
        require(!settledBatches[merkleRoot], "batch already settled");
        require(ethAmount > 0 || bnkrAmount > 0, "nothing to settle");
        require(address(this).balance >= ethAmount, "insufficient ETH");
        require(BNKR.balanceOf(address(this)) >= bnkrAmount, "insufficient BNKR");

        bytes32 leaf = keccak256(abi.encodePacked(merkleRoot, ethAmount, bnkrAmount, agentCount));
        require(MerkleProof.verify(proof, merkleRoot, leaf), "invalid merkle proof");

        settledBatches[merkleRoot] = true;
        lastSettledAt = block.timestamp;

        if (ethAmount > 0) {
            (bool ok, ) = payable(HELM_TREASURY).call{value: ethAmount}("");
            require(ok, "ETH treasury transfer failed");
            totalTreasuryEth += ethAmount;
        }
        if (bnkrAmount > 0) {
            BNKR.safeTransfer(HELM_TREASURY, bnkrAmount);
            totalTreasuryBnkr += bnkrAmount;
        }

        emit DailySettled(merkleRoot, ethAmount, bnkrAmount, agentCount, block.timestamp);
    }

    // ============================
    // YIELD (LIDO STAKING)
    // ============================

    function stakeIdleFunds() external onlyGateway {
        if (yieldProtocol == address(0)) return;
        uint256 idle = address(this).balance * stakeRatio / 100;
        if (idle == 0) return;
        (bool ok, ) = yieldProtocol.call{value: idle}(
            abi.encodeWithSignature("submit(address)", address(0))
        );
        require(ok, "Lido staking failed");
    }

    // ============================
    // VIEWS
    // ============================

    function getEthDeposit(address agent)  external view returns (uint256) { return ethDeposits[agent]; }
    function getBnkrDeposit(address agent) external view returns (uint256) { return bnkrDeposits[agent]; }
    function getEthTVL()                   external view returns (uint256) { return address(this).balance; }
    function getBnkrTVL()                  external view returns (uint256) { return BNKR.balanceOf(address(this)); }
    function nextSettleAt()                external view returns (uint256) { return lastSettledAt + SETTLE_INTERVAL; }
    function isRegistered(address agent)   external view returns (bool)    { return registeredDids[agent]; }
    function getReferrer(address agent)    external view returns (address)  { return referrer[agent]; }

    function treasuryStats() external view returns (
        uint256 eth_,
        uint256 bnkr_,
        uint256 referrerBnkr_,
        uint256 dids_
    ) {
        return (totalTreasuryEth, totalTreasuryBnkr, totalReferrerBnkr, totalDidsRegistered);
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
        require(newRatio <= 100, "ratio > 100");
        stakeRatio = newRatio;
    }

    function updateYieldProtocol(address newProtocol) external onlyOwner {
        yieldProtocol = newProtocol;
    }

    receive() external payable {
        ethDeposits[msg.sender] += msg.value;
        emit EthDeposited(msg.sender, msg.value);
    }
}
