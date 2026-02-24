// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// contracts/HelmSenseEscrow.sol
//
// [H-8] Helm-sense Gateway 에스크로 컨트랙트
// Base Chain (Ethereum L2) 배포 대상
//
// 취약점 수정 (Helm_INIt_Secure.txt):
//   Reentrancy → nonReentrant + CEI 패턴 (Checks-Effects-Interactions)
//   Front-running → Commit-Reveal (Phase 2 서명 기반)
//
// 역할:
//   Phase 1: 에이전트 예치금 관리
//   Phase 3: Gateway가 Merkle 증명으로 일괄 정산

import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/cryptography/MerkleProof.sol";

contract HelmSenseEscrow is ReentrancyGuard, Ownable {

    // ============================
    // STATE
    // ============================

    /// 에이전트 예치 잔액 (wei 단위 — ETH 기준)
    mapping(address => uint256) public deposits;

    /// 처리된 정산 배치 (재처리 방지)
    mapping(bytes32 => bool) public settledBatches;

    /// Gateway 주소 (정산 권한자)
    address public gateway;

    /// Treasury (수익 적립지)
    address public treasury;

    /// Staking Yield 컨트랙트 (Lido 연동)
    address public yieldProtocol;

    /// TVL 중 스테이킹 비율 (기본 80%)
    uint256 public stakeRatio = 80;

    // ============================
    // EVENTS
    // ============================

    event Deposited(address indexed agent, uint256 amount);
    event Withdrawn(address indexed agent, uint256 amount);
    event BatchSettled(bytes32 indexed merkleRoot, uint256 totalAmount, uint256 agentCount);
    event GatewayUpdated(address indexed oldGateway, address indexed newGateway);

    // ============================
    // MODIFIERS
    // ============================

    modifier onlyGateway() {
        require(msg.sender == gateway, "HelmSenseEscrow: caller is not gateway");
        _;
    }

    // ============================
    // CONSTRUCTOR
    // ============================

    constructor(
        address _gateway,
        address _treasury,
        address _yieldProtocol
    ) Ownable(msg.sender) {
        require(_gateway != address(0), "gateway cannot be zero");
        require(_treasury != address(0), "treasury cannot be zero");

        gateway = _gateway;
        treasury = _treasury;
        yieldProtocol = _yieldProtocol;
    }

    // ============================
    // PHASE 1: DEPOSIT
    // ============================

    /// 에이전트 예치 (ETH/BNKR)
    /// 가스비: 1회만 발생
    function deposit() external payable nonReentrant {
        require(msg.value > 0, "deposit amount must be positive");

        // [CEI] Effects before Interactions
        deposits[msg.sender] += msg.value;

        emit Deposited(msg.sender, msg.value);
    }

    /// 예치금 인출 (에이전트 자율)
    function withdraw(uint256 amount) external nonReentrant {
        // === CHECKS ===
        require(deposits[msg.sender] >= amount, "insufficient deposit");

        // === EFFECTS (상태 변경 먼저) ===
        deposits[msg.sender] -= amount;

        // === INTERACTIONS (외부 호출 마지막) ===
        (bool success, ) = payable(msg.sender).call{value: amount}("");
        require(success, "withdraw transfer failed");

        emit Withdrawn(msg.sender, amount);
    }

    // ============================
    // PHASE 3: BATCH SETTLEMENT
    // ============================

    /// 일괄 정산 (Gateway만 호출 가능)
    /// Merkle Root 1건으로 100K 티켓 처리
    ///
    /// @param merkleRoot  오프체인 티켓들의 Merkle Root
    /// @param totalAmount 총 정산 금액 (wei)
    /// @param agentCount  정산 대상 에이전트 수
    /// @param proof       Merkle 증명
    function settleBatch(
        bytes32 merkleRoot,
        uint256 totalAmount,
        uint256 agentCount,
        bytes32[] calldata proof
    ) external onlyGateway nonReentrant {
        // === CHECKS ===
        require(!settledBatches[merkleRoot], "batch already settled");
        require(totalAmount > 0, "settlement amount must be positive");
        require(address(this).balance >= totalAmount, "insufficient contract balance");

        // Merkle 증명 검증
        // (실제 운영: leaf = keccak256(merkleRoot, totalAmount, agentCount))
        bytes32 leaf = keccak256(abi.encodePacked(merkleRoot, totalAmount, agentCount));
        require(
            MerkleProof.verify(proof, merkleRoot, leaf),
            "invalid merkle proof"
        );

        // === EFFECTS ===
        settledBatches[merkleRoot] = true;

        // === INTERACTIONS ===
        (bool success, ) = payable(treasury).call{value: totalAmount}("");
        require(success, "treasury transfer failed");

        emit BatchSettled(merkleRoot, totalAmount, agentCount);
    }

    // ============================
    // YIELD (STAKING)
    // ============================

    /// 유휴 예치금 → Lido 스테이킹 (수익 극대화)
    /// Gateway가 주기적으로 호출
    function stakeIdleFunds() external onlyGateway {
        if (yieldProtocol == address(0)) return;

        uint256 idle = address(this).balance * stakeRatio / 100;
        if (idle == 0) return;

        // Lido submit (stETH 발행)
        (bool success, ) = yieldProtocol.call{value: idle}(
            abi.encodeWithSignature("submit(address)", address(0))
        );
        require(success, "staking failed");
    }

    // ============================
    // ADMIN
    // ============================

    /// Gateway 주소 업데이트 (Multisig로 보호)
    function updateGateway(address newGateway) external onlyOwner {
        require(newGateway != address(0), "invalid gateway");
        emit GatewayUpdated(gateway, newGateway);
        gateway = newGateway;
    }

    /// Stake 비율 조정 (0~100%)
    function updateStakeRatio(uint256 newRatio) external onlyOwner {
        require(newRatio <= 100, "ratio cannot exceed 100");
        stakeRatio = newRatio;
    }

    /// 에이전트 잔액 조회
    function getDeposit(address agent) external view returns (uint256) {
        return deposits[agent];
    }

    /// 컨트랙트 총 TVL
    function getTVL() external view returns (uint256) {
        return address(this).balance;
    }

    receive() external payable {
        deposits[msg.sender] += msg.value;
        emit Deposited(msg.sender, msg.value);
    }
}
