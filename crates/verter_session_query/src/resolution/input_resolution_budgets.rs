//! Immutable policy for one workspace-driven input-resolution operation.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputResolutionBudgetMeter {
    Attempts,
    UniqueKeys,
    InputBytes,
    DriverDepth,
    Churn,
    AliasGeometryRetention,
    CompletedWitnessRetention,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputResolutionBudgetError {
    meter: InputResolutionBudgetMeter,
    value: u64,
    ratified_maximum: u64,
}

impl InputResolutionBudgetError {
    #[must_use]
    pub const fn meter(self) -> InputResolutionBudgetMeter {
        self.meter
    }

    #[must_use]
    pub const fn value(self) -> u64 {
        self.value
    }

    #[must_use]
    pub const fn ratified_maximum(self) -> u64 {
        self.ratified_maximum
    }
}

/// The sole semantic-owned input-resolution budget policy carrier.
///
/// Values are inclusive maxima. An override is a complete immutable value and
/// may only tighten the ratified policy; zero never disables a meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputResolutionBudgets {
    attempts: u32,
    unique_keys: u32,
    input_bytes: u64,
    driver_depth: u32,
    churn: u32,
    alias_geometry_retention: u32,
    completed_witness_retention: u32,
}

impl InputResolutionBudgets {
    pub const RATIFIED: Self = Self {
        attempts: 256,
        unique_keys: 1_024,
        input_bytes: 1_048_576,
        driver_depth: 64,
        churn: 8,
        alias_geometry_retention: 1_024,
        completed_witness_retention: 1_024,
    };

    pub fn try_tightened(
        attempts: u32,
        unique_keys: u32,
        input_bytes: u64,
        driver_depth: u32,
        churn: u32,
    ) -> Result<Self, InputResolutionBudgetError> {
        validate(
            InputResolutionBudgetMeter::Attempts,
            attempts as u64,
            Self::RATIFIED.attempts as u64,
        )?;
        validate(
            InputResolutionBudgetMeter::UniqueKeys,
            unique_keys as u64,
            Self::RATIFIED.unique_keys as u64,
        )?;
        validate(
            InputResolutionBudgetMeter::InputBytes,
            input_bytes,
            Self::RATIFIED.input_bytes,
        )?;
        validate(
            InputResolutionBudgetMeter::DriverDepth,
            driver_depth as u64,
            Self::RATIFIED.driver_depth as u64,
        )?;
        validate(
            InputResolutionBudgetMeter::Churn,
            churn as u64,
            Self::RATIFIED.churn as u64,
        )?;
        Ok(Self {
            attempts,
            unique_keys,
            input_bytes,
            driver_depth,
            churn,
            alias_geometry_retention: Self::RATIFIED.alias_geometry_retention,
            completed_witness_retention: Self::RATIFIED.completed_witness_retention,
        })
    }

    pub fn try_tightened_with_retention(
        attempts: u32,
        unique_keys: u32,
        input_bytes: u64,
        driver_depth: u32,
        churn: u32,
        alias_geometry_retention: u32,
        completed_witness_retention: u32,
    ) -> Result<Self, InputResolutionBudgetError> {
        let base = Self::try_tightened(attempts, unique_keys, input_bytes, driver_depth, churn)?;
        validate(
            InputResolutionBudgetMeter::AliasGeometryRetention,
            u64::from(alias_geometry_retention),
            u64::from(Self::RATIFIED.alias_geometry_retention),
        )?;
        validate(
            InputResolutionBudgetMeter::CompletedWitnessRetention,
            u64::from(completed_witness_retention),
            u64::from(Self::RATIFIED.completed_witness_retention),
        )?;
        Ok(Self {
            alias_geometry_retention,
            completed_witness_retention,
            ..base
        })
    }

    #[must_use]
    pub const fn attempts(self) -> u32 {
        self.attempts
    }

    #[must_use]
    pub const fn unique_keys(self) -> u32 {
        self.unique_keys
    }

    #[must_use]
    pub const fn input_bytes(self) -> u64 {
        self.input_bytes
    }

    #[must_use]
    pub const fn driver_depth(self) -> u32 {
        self.driver_depth
    }

    #[must_use]
    pub const fn churn(self) -> u32 {
        self.churn
    }

    #[must_use]
    pub const fn alias_geometry_retention(self) -> u32 {
        self.alias_geometry_retention
    }

    #[must_use]
    pub const fn completed_witness_retention(self) -> u32 {
        self.completed_witness_retention
    }
}

impl Default for InputResolutionBudgets {
    fn default() -> Self {
        Self::RATIFIED
    }
}

fn validate(
    meter: InputResolutionBudgetMeter,
    value: u64,
    ratified_maximum: u64,
) -> Result<(), InputResolutionBudgetError> {
    if value == 0 || value > ratified_maximum {
        Err(InputResolutionBudgetError {
            meter,
            value,
            ratified_maximum,
        })
    } else {
        Ok(())
    }
}

/// One rejected prospective action, emitted before terminal escape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputResolutionBudgetExhaustion {
    pub meter: InputResolutionBudgetMeter,
    pub consumed: u64,
    pub prospective: u64,
    pub maximum: u64,
}
