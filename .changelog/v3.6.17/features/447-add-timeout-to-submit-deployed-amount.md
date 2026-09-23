- Add a mandatory `timeout` to the `SubmitDeployedAmount` execute message. Execution fails if the timeout is in the past, so stale DAO proposals can no longer be executed indefinitely.
  ([\#447](https://github.com/informalsystems/hydro/pull/447))
