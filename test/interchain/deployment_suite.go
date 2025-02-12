package interchain

import (
	"context"
	"encoding/json"
	"hydro/test/interchain/chainsuite"
	"os"
	"path"
	"strconv"

	wasmtypes "github.com/CosmWasm/wasmd/x/wasm/types"
)

var (
	// astroport
	astroFactoryCodeId int
	astroPairCodeId    int
	astroTokenCodeId   int
	factoryAddr        string
	astroTokenAAddr    string
	astroTokenBAddr    string
	poolTokenATokenB   string
	// valence
	valenceAuthCodeId      int
	valenceProcessorCodeId int
	valenceProcessorAddr   string
	valenceAuthAddress     string
)

const (
	valenceAuthCodeHash = "71ac0e115741f9f16603dbbce97ffe93d7c90f62a85d3b96a98ac27987da0fc2"
	salt                = "617574686f72697a6174696f6e" // hex encoded "authorization" string
)

type DeploymentSuite struct {
	HydroSuite
}

func (s *DeploymentSuite) SetupSuite() {
	ctx, err := chainsuite.NewSuiteContext(&s.Suite)
	s.Require().NoError(err)
	s.ctx = ctx

	// create and start hub chain
	s.HubChain, err = chainsuite.CreateChain(s.GetContext(), s.T(), chainsuite.GetHubSpec(4))
	s.Require().NoError(err)

	// setup hermes relayer
	relayer, err := chainsuite.NewRelayer(s.GetContext(), s.T())
	s.Require().NoError(err)
	s.Relayer = relayer
	err = relayer.SetupChainKeys(s.GetContext(), s.HubChain)
	s.Require().NoError(err)

	// create and start neutron chain
	s.NeutronChain, err = s.HubChain.AddConsumerChain(s.GetContext(), relayer, 4, chainsuite.NeutronChainID, chainsuite.GetNeutronSpec)
	s.Require().NoError(err)
	s.Require().NoError(s.HubChain.UpdateAndVerifyStakeChange(s.GetContext(), s.NeutronChain, relayer, 1_000_000, 0))

	// Astroport setup
	s.storeAstroportContracts()
	s.initFactoryAndCreateTestPair()
	s.ProvideLiquidityForTokenPair(poolTokenATokenB, astroTokenAAddr, astroTokenBAddr, "5000")

	// Valence setup
	s.storeValenceContracts()
	s.initValenceContracts()

}

func (s *DeploymentSuite) GetContext() context.Context {
	s.Require().NotNil(s.ctx, "Tried to GetContext before it was set. SetupSuite must run first")
	return s.ctx
}

func (s *DeploymentSuite) getAstroportFactoryContractPath() string {
	return path.Join(s.NeutronChain.GetNode().HomeDir(), "astroport_factory_cw20.wasm")
}

func (s *DeploymentSuite) getAstroportPairContractPath() string {
	return path.Join(s.NeutronChain.GetNode().HomeDir(), "astroport_pair_cw20.wasm")
}

func (s *DeploymentSuite) getAstroportTokenContractPath() string {
	return path.Join(s.NeutronChain.GetNode().HomeDir(), "astroport_token.wasm")
}

func (s *DeploymentSuite) getValenceAuthContractPath() string {
	return path.Join(s.NeutronChain.GetNode().HomeDir(), "valence_authorization.wasm")
}

func (s *DeploymentSuite) getValenceProcessorContractPath() string {
	return path.Join(s.NeutronChain.GetNode().HomeDir(), "valence_processor.wasm")
}

func (s *DeploymentSuite) storeAstroportContracts() {
	// token factory
	astroFactoryContract, err := os.ReadFile("./testdata/astroport_factory_cw20.wasm")
	s.Require().NoError(err)
	s.Require().NoError(s.NeutronChain.GetNode().WriteFile(s.GetContext(), astroFactoryContract, "astroport_factory_cw20.wasm"))
	astroFactoryCodeId, err = strconv.Atoi(s.StoreCode(s.getAstroportFactoryContractPath(), chainsuite.AdminMoniker))
	s.Require().NoError(err)
	// token pair
	astroPairContract, err := os.ReadFile("./testdata/astroport_pair_cw20.wasm")
	s.Require().NoError(err)
	s.Require().NoError(s.NeutronChain.GetNode().WriteFile(s.GetContext(), astroPairContract, "astroport_pair_cw20.wasm"))
	astroPairCodeId, err = strconv.Atoi(s.StoreCode(s.getAstroportPairContractPath(), chainsuite.AdminMoniker))
	s.Require().NoError(err)
	// token
	astroTokenContract, err := os.ReadFile("./testdata/astroport_token.wasm")
	s.Require().NoError(err)
	s.Require().NoError(s.NeutronChain.GetNode().WriteFile(s.GetContext(), astroTokenContract, "astroport_token.wasm"))
	astroTokenCodeId, err = strconv.Atoi(s.StoreCode(s.getAstroportTokenContractPath(), chainsuite.AdminMoniker))
	s.Require().NoError(err)
}

func (s *DeploymentSuite) storeValenceContracts() {
	// authorization contract
	authContract, err := os.ReadFile("./testdata/valence_authorization.wasm")
	s.Require().NoError(err)
	s.Require().NoError(s.NeutronChain.GetNode().WriteFile(s.GetContext(), authContract, "valence_authorization.wasm"))
	valenceAuthCodeId, err = strconv.Atoi(s.StoreCode(s.getValenceAuthContractPath(), chainsuite.AdminMoniker))
	s.Require().NoError(err)

	// processor contract
	processorContract, err := os.ReadFile("./testdata/valence_processor.wasm")
	s.Require().NoError(err)
	s.Require().NoError(s.NeutronChain.GetNode().WriteFile(s.GetContext(), processorContract, "valence_processor.wasm"))
	valenceProcessorCodeId, err = strconv.Atoi(s.StoreCode(s.getValenceProcessorContractPath(), chainsuite.AdminMoniker))
	s.Require().NoError(err)
}

func (s *DeploymentSuite) initFactoryAndCreateTestPair() {
	s.Suite.T().Log("Instantiating Astroport contracts")
	adminAddr := chainsuite.NeutronAdminAddress
	balances := []map[string]string{
		{
			"address": adminAddr,
			"amount":  "1000000000",
		},
	}
	astroTokenAAddr = s.InitTokenContract("Token A", "TKNA", adminAddr, balances)
	astroTokenBAddr = s.InitTokenContract("Token B", "TKNB", adminAddr, balances)
	factoryAddr = s.InitAstroFactoryContract(adminAddr)

	s.Suite.T().Log("Create token A and token B pair")
	poolTokenATokenB = s.CreateTokenPair(factoryAddr, astroTokenAAddr, astroTokenBAddr)
}

func (s *DeploymentSuite) initValenceContracts() {
	admin := chainsuite.NeutronAdminAddress
	//predictedValenceAuthAddress := s.PredictContractAddress(valenceAuthCodeHash, admin, salt)
	// build-address does not accept --node flag and until its fixed we're using hardcoded predicted address
	predictedValenceAuthAddress := "neutron1fsr30ry0rgu8zm0e2dlpmpn4fj9m4p782kgnp08qe7evzu4m2e3scecfq9"
	valenceProcessorAddr = s.InitValenceProcessor(predictedValenceAuthAddress, admin)
	valenceAuthAddress = s.InitValenceAuthorization(valenceProcessorAddr, admin)
	s.Require().Equal(predictedValenceAuthAddress, valenceAuthAddress)
}

func (s *DeploymentSuite) ProvideLiquidityForTokenPair(poolAddr, tokenAAddr, tokenBAddr, amount string) {
	s.IncreaseTokenAllowance(tokenAAddr, poolAddr, amount) // we're using same amount for both token, if needed this can be changed
	s.IncreaseTokenAllowance(tokenBAddr, poolAddr, amount)
	s.ProvideLiquidity(poolAddr, tokenAAddr, tokenBAddr, amount)
}

func (s *DeploymentSuite) InitTokenContract(name, symbol, adminAddr string, balances []map[string]string) string {
	initToken := map[string]interface{}{
		"name":             name,
		"symbol":           symbol,
		"decimals":         6,
		"initial_balances": balances,
	}

	initTokenJson, err := json.Marshal(initToken)
	s.Require().NoError(err)

	txHash, err := s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		chainsuite.AdminMoniker,
		"wasm", "instantiate", strconv.Itoa(astroTokenCodeId), string(initTokenJson), "--label", name, "--admin", adminAddr, "--gas", "auto",
	)
	s.Require().NoError(err)
	response, err := s.NeutronChain.Validators[0].TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)
	contractAddr, found := getEvtAttribute(response.Events, wasmtypes.EventTypeInstantiate, wasmtypes.AttributeKeyContractAddr)
	s.Require().True(found)

	return contractAddr
}

func (s *DeploymentSuite) InitAstroFactoryContract(ownerAddr string) string {
	initFactory := map[string]interface{}{
		"owner": ownerAddr,
		"pair_configs": []map[string]interface{}{
			{
				"code_id": astroPairCodeId,
				"pair_type": map[string]interface{}{
					"xyk": struct{}{},
				},
				"total_fee_bps":         0,
				"maker_fee_bps":         0,
				"is_disabled":           false,
				"is_generator_disabled": true,
			},
		},
		"token_code_id":         astroTokenCodeId,
		"whitelist_code_id":     0,
		"coin_registry_address": ownerAddr,
	}

	initFactoryJson, err := json.Marshal(initFactory)
	s.Require().NoError(err)

	txHash, err := s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		chainsuite.AdminMoniker,
		"wasm", "instantiate", strconv.Itoa(astroFactoryCodeId), string(initFactoryJson), "--label", "Astroport Factory", "--admin", ownerAddr, "--gas", "auto",
	)
	s.Require().NoError(err)
	response, err := s.NeutronChain.Validators[0].TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)
	contractAddr, found := getEvtAttribute(response.Events, wasmtypes.EventTypeInstantiate, wasmtypes.AttributeKeyContractAddr)
	s.Require().True(found)

	return contractAddr
}

func (s *DeploymentSuite) CreateTokenPair(tokenFactoryAddr, tokenAAddr, tokenBAddr string) string {
	createPairMsg := map[string]interface{}{
		"create_pair": map[string]interface{}{
			"pair_type": map[string]interface{}{
				"xyk": struct{}{},
			},
			"asset_infos": []map[string]map[string]string{
				{"token": {"contract_addr": tokenAAddr}},
				{"token": {"contract_addr": tokenBAddr}},
			},
		},
	}

	tokenPairJson, err := json.Marshal(createPairMsg)
	s.Require().NoError(err)

	txHash, err := s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		chainsuite.AdminMoniker,
		"wasm", "execute", tokenFactoryAddr, string(tokenPairJson), "--gas", "auto",
	)
	s.Require().NoError(err)
	response, err := s.NeutronChain.Validators[0].TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)
	contractAddr, found := getEvtAttribute(response.Events, wasmtypes.EventTypeInstantiate, wasmtypes.AttributeKeyContractAddr)
	s.Require().True(found)

	return contractAddr
}

func (s *DeploymentSuite) IncreaseTokenAllowance(tokenAddr, spenderAddr, amount string) {
	msg := map[string]interface{}{
		"increase_allowance": map[string]interface{}{
			"spender": spenderAddr,
			"amount":  amount,
		},
	}

	msgJson, err := json.Marshal(msg)
	s.Require().NoError(err)

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		chainsuite.AdminMoniker,
		"wasm", "execute", tokenAddr, string(msgJson), "--gas", "auto",
	)
	s.Require().NoError(err)
}

func (s *DeploymentSuite) ProvideLiquidity(poolAddr, tokenAAddr, tokenBAddr, amount string) {
	msg := map[string]interface{}{
		"provide_liquidity": map[string]interface{}{
			"assets": []map[string]interface{}{
				{
					"info": map[string]interface{}{
						"token": map[string]interface{}{
							"contract_addr": tokenAAddr,
						},
					},
					"amount": amount,
				},
				{
					"info": map[string]interface{}{
						"token": map[string]interface{}{
							"contract_addr": tokenBAddr,
						},
					},
					"amount": amount,
				},
			},
			"slippage_tolerance": "0.01",
			"auto_stake":         false,
		},
	}

	msgJson, err := json.Marshal(msg)
	s.Require().NoError(err)

	_, err = s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		chainsuite.AdminMoniker,
		"wasm", "execute", poolAddr, string(msgJson), "--gas", "auto",
	)
	s.Require().NoError(err)
}

func (s *DeploymentSuite) PredictContractAddress(codeHash, creatorAddress, hexEncodedSalt string) string {
	node := s.NeutronChain.Validators[0]
	command := []string{"query", "wasm", "build-address", codeHash, creatorAddress, hexEncodedSalt}
	command = node.NodeCommand(command...)
	response, _, err := node.Exec(s.GetContext(), command, node.Chain.Config().Env)
	s.Require().NoError(err)

	var address chainsuite.ContractAddress
	err = json.Unmarshal([]byte(response), &address)
	s.Require().NoError(err)

	return address.Address
}

func (s *DeploymentSuite) InitValenceProcessor(predictedAuthAddress, adminAddr string) string {
	initProcessor := map[string]interface{}{
		"authorization_contract": predictedAuthAddress,
	}

	initProcessorJson, err := json.Marshal(initProcessor)
	s.Require().NoError(err)

	txHash, err := s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		chainsuite.AdminMoniker,
		"wasm", "instantiate", strconv.Itoa(valenceProcessorCodeId), string(initProcessorJson), "--label", "Proccessor", "--admin", adminAddr, "--gas", "auto",
	)
	s.Require().NoError(err)
	response, err := s.NeutronChain.Validators[0].TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)
	contractAddr, found := getEvtAttribute(response.Events, wasmtypes.EventTypeInstantiate, wasmtypes.AttributeKeyContractAddr)
	s.Require().True(found)

	return contractAddr
}

func (s *DeploymentSuite) InitValenceAuthorization(processorAddr, ownerAddr string) string {
	initAuthorization := map[string]interface{}{
		"owner":      ownerAddr,
		"processor":  processorAddr,
		"sub_owners": []string{},
	}

	initAuthorizationJson, err := json.Marshal(initAuthorization)
	s.Require().NoError(err)

	txHash, err := s.NeutronChain.Validators[0].ExecTx(
		s.GetContext(),
		chainsuite.AdminMoniker,
		"wasm", "instantiate2", strconv.Itoa(valenceAuthCodeId), string(initAuthorizationJson), salt, "--label", "Authorization", "--admin", ownerAddr, "--gas", "auto",
	)
	s.Require().NoError(err)
	response, err := s.NeutronChain.Validators[0].TxHashToResponse(s.GetContext(), txHash)
	s.Require().NoError(err)
	contractAddr, found := getEvtAttribute(response.Events, wasmtypes.EventTypeInstantiate, wasmtypes.AttributeKeyContractAddr)
	s.Require().True(found)

	return contractAddr
}
