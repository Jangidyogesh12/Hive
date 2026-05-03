from __future__ import annotations


class Node:
    left: Node | None
    right: Node | None

    def __init__(self, val):
        self.val = val
        self.right = None
        self.left = None
        self.height = 0


class Tree:
    def __init__(self, root) -> None:
        self.root = Node(root)

    def _height(self, node):
        return node.height if node else -1

    def _updata_height(self, node):
        node.height = 1 + max(self._height(node.left), self._height(node.right))

    def _balance_factor(self, node):
        return self._height(node.left) - self._height(node.right)

    def right_rotate(self, y):
        x = y.left
        t2 = x.right

        x.right = y
        y.left = t2

        self._updata_height(x)
        self._updata_height(y)

        return y

    def left_rotate(self, x):
        y = x.right
        t2 = x.left

        y.left = x
        x.right = t2

        self._updata_height(x)
        self._updata_height(y)

        return x

    def insert(self, root, node_val):
        if not root:
            return Node(node_val)

        if node_val < root.val:
            root.left = self.insert(root.left, node_val)
        elif node_val > root.val:
            root.right = self.insert(root.right, node_val)
        else:
            return root

        self._updata_height(root)

        bf = self._balance_factor(root)

        if bf > 1 and root.left is not None and node_val < root.left.val:
            return self.right_rotate(root)

        if bf < -1 and root.right is not None and node_val > root.right.val:
            return self.left_rotate(root)

        if bf > 1 and root.left is not None and node_val > root.left.val:
            root.left = self.left_rotate(root.left)
            return self.right_rotate(root)

        if bf < -1 and root.right is not None and node_val < root.right.val:
            root.right = self.right_rotate(root.right)
            return self.left_rotate(root)

        return root

    def traverse(self, start) -> None:
        if not start:
            return
        self.traverse(start.left)
        print(start.val)
        self.traverse(start.right)


b_tree = Tree(4)
