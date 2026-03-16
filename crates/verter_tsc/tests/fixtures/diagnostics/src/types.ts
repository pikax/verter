export interface User {
  id: number
  name: string
  email: string
  age: number
}

export interface Product {
  id: number
  title: string
  price: number
  inStock: boolean
}

export type Status = 'active' | 'inactive' | 'pending'

export type Theme = 'light' | 'dark'

export interface PaginatedResult<T> {
  items: T[]
  total: number
  page: number
}

export interface FormField {
  label: string
  value: string | number
  required: boolean
}
