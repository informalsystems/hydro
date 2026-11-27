- Add the h token info provider contract, which reads the exchange rate from the
  h token contract's `rate` query and reports it to Hydro, so that h token
  lockups (such as hATOM on the Cosmos Hub) can be valued.
