//! Operation composition types for chaining, batching, and conditional execution
//!
//! This module provides types and methods for composing UPnP operations:
//! - Sequential execution (and_then)
//! - Parallel execution (concurrent_with)
//! - Conditional execution (condition)

use super::{ComposableOperation, UPnPOperation};

/// A sequence of operations to be executed in order
///
/// OperationSequence represents a chain of operations where each operation
/// executes after the previous one completes. The output of each operation
/// can be used to determine whether to continue with the next operation.
///
/// # Type Parameters
/// * `Ops` - A tuple type representing the operations in the sequence
pub struct OperationSequence<Ops> {
    operations: Ops,
}

/// A batch of operations to be executed concurrently
///
/// OperationBatch represents a group of operations that can be executed
/// in parallel. All operations in the batch are independent and can run
/// simultaneously without conflicts.
///
/// # Type Parameters
/// * `Ops` - A tuple type representing the operations in the batch
pub struct OperationBatch<Ops> {
    operations: Ops,
}

/// A conditional operation that executes based on a predicate
///
/// ConditionalOperation wraps an operation with a condition that determines
/// whether the operation should be executed based on some criteria.
///
/// # Type Parameters
/// * `Op` - The UPnP operation type
/// * `F` - The predicate function type
pub struct ConditionalOperation<Op: UPnPOperation, F> {
    operation: ComposableOperation<Op>,
    predicate: F,
}

// Implementation for ComposableOperation to add composition methods
impl<Op: UPnPOperation> ComposableOperation<Op> {
    /// Chain this operation with another operation to execute sequentially
    ///
    /// The second operation will execute only after this operation completes
    /// successfully. The sequence will fail if any operation in the chain fails.
    ///
    /// # Type Parameters
    /// * `Op2` - The type of the second operation
    ///
    /// # Arguments
    /// * `next` - The operation to execute after this one
    ///
    /// # Returns
    /// An OperationSequence containing both operations
    pub fn and_then<Op2: UPnPOperation>(
        self,
        next: ComposableOperation<Op2>,
    ) -> OperationSequence<(ComposableOperation<Op>, ComposableOperation<Op2>)> {
        OperationSequence {
            operations: (self, next),
        }
    }

    /// Execute this operation concurrently with another operation
    ///
    /// Both operations will be executed in parallel. The batch will succeed
    /// only if all operations succeed.
    ///
    /// # Type Parameters
    /// * `Op2` - The type of the second operation
    ///
    /// # Arguments
    /// * `other` - The operation to execute concurrently
    ///
    /// # Returns
    /// An OperationBatch containing both operations
    pub fn concurrent_with<Op2: UPnPOperation>(
        self,
        other: ComposableOperation<Op2>,
    ) -> OperationBatch<(ComposableOperation<Op>, ComposableOperation<Op2>)> {
        // Check if operations can be safely batched
        if !Op::can_batch_with::<Op2>() {
            eprintln!(
                "Warning: Operations {} and {} may not be safe to batch together",
                Op::ACTION,
                Op2::ACTION
            );
        }

        OperationBatch {
            operations: (self, other),
        }
    }

    /// Make this operation conditional based on a predicate
    ///
    /// The operation will only execute if the predicate returns true.
    /// This is useful for operations that should only run under certain conditions.
    ///
    /// # Type Parameters
    /// * `F` - The predicate function type
    ///
    /// # Arguments
    /// * `predicate` - A function that determines whether to execute the operation
    ///
    /// # Returns
    /// A ConditionalOperation with the predicate
    pub fn condition<F>(self, predicate: F) -> ConditionalOperation<Op, F>
    where
        F: Fn() -> bool,
    {
        ConditionalOperation {
            operation: self,
            predicate,
        }
    }

    /// Make this operation conditional based on a previous operation's response
    ///
    /// This creates a conditional operation that depends on the result of
    /// another operation. This is useful for chaining operations where the
    /// next operation depends on the result of the previous one.
    ///
    /// # Type Parameters
    /// * `PrevOp` - The type of the previous operation
    /// * `F` - The predicate function type
    ///
    /// # Arguments
    /// * `predicate` - A function that takes the previous response and returns bool
    ///
    /// # Returns
    /// A ConditionalOperation with the response-based predicate
    pub fn condition_on_response<PrevOp: UPnPOperation, F>(
        self,
        predicate: F,
    ) -> ConditionalOperation<Op, F>
    where
        F: Fn(&PrevOp::Response) -> bool,
    {
        ConditionalOperation {
            operation: self,
            predicate,
        }
    }
}

// Implementation for OperationSequence
impl<Op1: UPnPOperation, Op2: UPnPOperation> OperationSequence<(ComposableOperation<Op1>, ComposableOperation<Op2>)> {
    /// Add another operation to the end of this sequence
    ///
    /// # Type Parameters
    /// * `Op3` - The type of the operation to add
    ///
    /// # Arguments
    /// * `next` - The operation to add to the sequence
    ///
    /// # Returns
    /// A new OperationSequence with the additional operation
    pub fn and_then<Op3: UPnPOperation>(
        self,
        next: ComposableOperation<Op3>,
    ) -> OperationSequence<(ComposableOperation<Op1>, ComposableOperation<Op2>, ComposableOperation<Op3>)> {
        OperationSequence {
            operations: (self.operations.0, self.operations.1, next),
        }
    }

    /// Get the operations in this sequence
    pub fn operations(&self) -> &(ComposableOperation<Op1>, ComposableOperation<Op2>) {
        &self.operations
    }

    /// Convert this sequence into its component operations
    pub fn into_operations(self) -> (ComposableOperation<Op1>, ComposableOperation<Op2>) {
        self.operations
    }
}

// Implementation for OperationBatch
impl<Op1: UPnPOperation, Op2: UPnPOperation> OperationBatch<(ComposableOperation<Op1>, ComposableOperation<Op2>)> {
    /// Add another operation to this batch
    ///
    /// # Type Parameters
    /// * `Op3` - The type of the operation to add
    ///
    /// # Arguments
    /// * `other` - The operation to add to the batch
    ///
    /// # Returns
    /// A new OperationBatch with the additional operation
    pub fn concurrent_with<Op3: UPnPOperation>(
        self,
        other: ComposableOperation<Op3>,
    ) -> OperationBatch<(ComposableOperation<Op1>, ComposableOperation<Op2>, ComposableOperation<Op3>)> {
        // Check batch compatibility with all existing operations
        if !Op1::can_batch_with::<Op3>() || !Op2::can_batch_with::<Op3>() {
            eprintln!(
                "Warning: Operation {} may not be safe to batch with existing operations",
                Op3::ACTION
            );
        }

        OperationBatch {
            operations: (self.operations.0, self.operations.1, other),
        }
    }

    /// Get the operations in this batch
    pub fn operations(&self) -> &(ComposableOperation<Op1>, ComposableOperation<Op2>) {
        &self.operations
    }

    /// Convert this batch into its component operations
    pub fn into_operations(self) -> (ComposableOperation<Op1>, ComposableOperation<Op2>) {
        self.operations
    }
}

// Implementation for ConditionalOperation
impl<Op: UPnPOperation, F> ConditionalOperation<Op, F> {
    /// Get the underlying operation
    pub fn operation(&self) -> &ComposableOperation<Op> {
        &self.operation
    }

    /// Get a reference to the predicate function
    pub fn predicate(&self) -> &F {
        &self.predicate
    }

    /// Check if the operation should execute based on the predicate
    pub fn should_execute(&self) -> bool
    where
        F: Fn() -> bool,
    {
        (self.predicate)()
    }

    /// Chain this conditional operation with another operation
    ///
    /// If the condition is met and this operation executes, the next operation
    /// will also execute. If the condition is not met, the entire sequence is skipped.
    ///
    /// # Type Parameters
    /// * `Op2` - The type of the next operation
    ///
    /// # Arguments
    /// * `next` - The operation to execute after this conditional operation
    ///
    /// # Returns
    /// An OperationSequence containing the conditional and next operations
    pub fn and_then<Op2: UPnPOperation>(
        self,
        next: ComposableOperation<Op2>,
    ) -> OperationSequence<(ConditionalOperation<Op, F>, ComposableOperation<Op2>)> {
        OperationSequence {
            operations: (self, next),
        }
    }

    /// Convert this conditional operation into its components
    pub fn into_parts(self) -> (ComposableOperation<Op>, F) {
        (self.operation, self.predicate)
    }
}

// Debug implementations
impl<Ops> std::fmt::Debug for OperationSequence<Ops>
where
    Ops: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationSequence")
            .field("operations", &self.operations)
            .finish()
    }
}

impl<Ops> std::fmt::Debug for OperationBatch<Ops>
where
    Ops: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationBatch")
            .field("operations", &self.operations)
            .finish()
    }
}

impl<Op: UPnPOperation, F> std::fmt::Debug for ConditionalOperation<Op, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConditionalOperation")
            .field("operation", &self.operation)
            .field("predicate", &"<function>")
            .finish()
    }
}

// Clone implementations where possible
impl<Ops: Clone> Clone for OperationSequence<Ops> {
    fn clone(&self) -> Self {
        Self {
            operations: self.operations.clone(),
        }
    }
}

impl<Ops: Clone> Clone for OperationBatch<Ops> {
    fn clone(&self) -> Self {
        Self {
            operations: self.operations.clone(),
        }
    }
}

impl<Op: UPnPOperation, F: Clone> Clone for ConditionalOperation<Op, F>
where
    Op::Request: Clone,
{
    fn clone(&self) -> Self {
        Self {
            operation: self.operation.clone(),
            predicate: self.predicate.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation::{OperationBuilder, ValidationLevel, ValidationError, Validate};
    use crate::service::Service;
    use serde::{Serialize, Deserialize};
    use xmltree::Element;

    // Mock types for testing
    #[derive(Serialize, Clone, Debug, PartialEq)]
    struct TestRequest {
        value: i32,
    }

    impl Validate for TestRequest {}

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestResponse {
        result: String,
    }

    struct TestOperation1;
    struct TestOperation2;

    impl UPnPOperation for TestOperation1 {
        type Request = TestRequest;
        type Response = TestResponse;
        const SERVICE: Service = Service::AVTransport;
        const ACTION: &'static str = "TestAction1";

        fn build_payload(request: &Self::Request) -> Result<String, ValidationError> {
            Ok(format!("<Test1>{}</Test1>", request.value))
        }

        fn parse_response(_xml: &Element) -> Result<Self::Response, crate::error::ApiError> {
            Ok(TestResponse { result: "test1".to_string() })
        }
    }

    impl UPnPOperation for TestOperation2 {
        type Request = TestRequest;
        type Response = TestResponse;
        const SERVICE: Service = Service::RenderingControl;
        const ACTION: &'static str = "TestAction2";

        fn build_payload(request: &Self::Request) -> Result<String, ValidationError> {
            Ok(format!("<Test2>{}</Test2>", request.value))
        }

        fn parse_response(_xml: &Element) -> Result<Self::Response, crate::error::ApiError> {
            Ok(TestResponse { result: "test2".to_string() })
        }
    }

    #[test]
    fn test_operation_sequence_and_then() {
        let op1 = OperationBuilder::<TestOperation1>::new(TestRequest { value: 1 })
            .build()
            .unwrap();
        let op2 = OperationBuilder::<TestOperation2>::new(TestRequest { value: 2 })
            .build()
            .unwrap();

        let sequence = op1.and_then(op2);
        let operations = sequence.operations();

        assert_eq!(operations.0.request().value, 1);
        assert_eq!(operations.1.request().value, 2);
    }

    #[test]
    fn test_operation_batch_concurrent_with() {
        let op1 = OperationBuilder::<TestOperation1>::new(TestRequest { value: 1 })
            .build()
            .unwrap();
        let op2 = OperationBuilder::<TestOperation2>::new(TestRequest { value: 2 })
            .build()
            .unwrap();

        let batch = op1.concurrent_with(op2);
        let operations = batch.operations();

        assert_eq!(operations.0.request().value, 1);
        assert_eq!(operations.1.request().value, 2);
    }

    #[test]
    fn test_conditional_operation() {
        let op = OperationBuilder::<TestOperation1>::new(TestRequest { value: 42 })
            .build()
            .unwrap();

        let conditional = op.condition(|| true);
        assert!(conditional.should_execute());

        let conditional_false = OperationBuilder::<TestOperation1>::new(TestRequest { value: 42 })
            .build()
            .unwrap()
            .condition(|| false);
        assert!(!conditional_false.should_execute());
    }

    #[test]
    fn test_conditional_and_then() {
        let op1 = OperationBuilder::<TestOperation1>::new(TestRequest { value: 1 })
            .build()
            .unwrap();
        let op2 = OperationBuilder::<TestOperation2>::new(TestRequest { value: 2 })
            .build()
            .unwrap();

        let conditional_sequence = op1.condition(|| true).and_then(op2);

        // Verify the sequence structure
        let debug_str = format!("{:?}", conditional_sequence);
        assert!(debug_str.contains("ConditionalOperation"));
        assert!(debug_str.contains("TestAction1"));
    }

    #[test]
    fn test_sequence_chaining() {
        let op1 = OperationBuilder::<TestOperation1>::new(TestRequest { value: 1 })
            .build()
            .unwrap();
        let op2 = OperationBuilder::<TestOperation2>::new(TestRequest { value: 2 })
            .build()
            .unwrap();
        let op3 = OperationBuilder::<TestOperation1>::new(TestRequest { value: 3 })
            .build()
            .unwrap();

        let sequence = op1.and_then(op2).and_then(op3);

        // Test that we can access the operations
        let debug_str = format!("{:?}", sequence);
        assert!(debug_str.contains("OperationSequence"));
    }

    #[test]
    fn test_batch_expansion() {
        let op1 = OperationBuilder::<TestOperation1>::new(TestRequest { value: 1 })
            .build()
            .unwrap();
        let op2 = OperationBuilder::<TestOperation2>::new(TestRequest { value: 2 })
            .build()
            .unwrap();
        let op3 = OperationBuilder::<TestOperation1>::new(TestRequest { value: 3 })
            .build()
            .unwrap();

        let batch = op1.concurrent_with(op2).concurrent_with(op3);

        // Test that we can create the batch
        let debug_str = format!("{:?}", batch);
        assert!(debug_str.contains("OperationBatch"));
    }

    #[test]
    fn test_composition_debug() {
        let op1 = OperationBuilder::<TestOperation1>::new(TestRequest { value: 1 })
            .build()
            .unwrap();
        let op2 = OperationBuilder::<TestOperation2>::new(TestRequest { value: 2 })
            .build()
            .unwrap();

        let sequence = op1.and_then(op2);
        let debug_str = format!("{:?}", sequence);
        assert!(debug_str.contains("OperationSequence"));

        let op3 = OperationBuilder::<TestOperation1>::new(TestRequest { value: 3 })
            .build()
            .unwrap();
        let op4 = OperationBuilder::<TestOperation2>::new(TestRequest { value: 4 })
            .build()
            .unwrap();

        let batch = op3.concurrent_with(op4);
        let debug_str = format!("{:?}", batch);
        assert!(debug_str.contains("OperationBatch"));
    }
}