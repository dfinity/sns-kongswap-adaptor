# What is a DEX?

A Decentralized EXchange (DEX) is a smart contract that enables users to trade tokens without relying on a centralized intermediary. Instead, trades are executed directly on-chain through liquidity pools.

# Liquidity Pool:

Each DEX consists of one or more liquidity pools. A pool typically holds two tokens (say, $T_0$ and $T_1$). 
In an ideal world, there would be a pool for every possible token pair.
Role of the users interacting with a liquidity pool can be:

1. Liquidity Providers (LPs): Users who deposit tokens into pools are called liquidity providers.
2. Traders: Users who swap one token for another. The exchange rate is determined by the ratio of tokens in the pool.
3. Platform providers: Owners of the DEX.


When a trade occurs, the trader pays a fee. This fee usually consists of:
1. A platform fee that goes to the DEX protocol, and
2. An LP fee that is distributed to liquidity providers, proportional to the amount of liquidity they contributed.

Some platforms allow LPs to claim and reinvest their fees, effectively compounding their rewards.

# Market Ratio:

A liquidity pool maintains a market ratio (MR) between its two tokens:

$MR = \frac{reserve_0}{reserve_1}$

When an LP deposits $amount_0$ and $amount_1$ to the pool (deposit ratio $DR = \frac{amount_0}{amount_1}$), pool takes from $amount_0$ and $amount_1$ according to the market ratio:

1. if $DR == MR$ => pool takes $amount_0$ and $amount_1$
2. if $DR >  MR$ => pool takes $amount_1 \times MR$ from $amount_0$ and $amount_0$
3. if $DR <  MR$ => pool takes $amount_0$ and $amount_0 \times MR$ from $amount_1$

This ensures that liquidity is always added proportional to the current reserves' balances.
If $DR \ne MR$, the excess amount is retunred to the LP.

# Trading:

Assume a trader wants to swap $T_0$ for $T_1$. To trade, they sends $amount_0$ of $T_0$.

In the uniform liquidity model (used in Uniswap V2), liquidity is defined as:

$L^2 = reserve_0 * reserve_1$

This formula is know as *Constant Product Invariant*. When trading, the liquidity remains unchanged:

$reserve_0 * reserve_1 = (reserve_0 + amount_0) * (reserve_1 - amount_1)$

$amount_1$ is the amount of $T_1$ that is going to be removed from the pool. Before sending
it to the user, some fees are deducted: mainly a platform fee that goes to the owners of the
DEX and an LP-fee that is going to be accumulated for the liquidity porivders proportional to the 
amount of liquidity they have added to the system.

# Withdrawing Liquidity:

Liquidity providers can withdraw their tokens at any time. However, due to trades happening in between, they rarely receive the same token amounts they initially deposited. Instead, they withdraw, depending on the net direction of trades:

1. More of one token,
2. Less of the other, and
3. The fees they have accrued during trading activity.

Some DEXes provide the LPs with the functionality to withdraw their accrued fee and, if desired, reinvest it to
the protocol to compound their rewards.