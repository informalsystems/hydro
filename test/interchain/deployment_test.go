package interchain

import (
	"testing"
	"time"

	"github.com/stretchr/testify/suite"
)

func TestDeploymentSuite(t *testing.T) {
	s := &DeploymentSuite{}
	suite.Run(t, s)
}

func (s *HydroSuite) TestDeployment() {
	// todo: add testing scenario (sleep is added to be able to test setup and manual cli calls on nodes)
	time.Sleep(time.Hour)
}
